use crate::{select, Candidate, Sample};
use ::windows::Win32::{
    NetworkManagement::{
        IpHelper::{
            FreeMibTable, GetIfTable2, GetIpForwardTable2, GetIpInterfaceEntry,
            InitializeIpInterfaceEntry, MIB_IF_TABLE2, MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::AF_UNSPEC,
};

struct Table<T>(*mut T);
impl<T> Drop for Table<T> {
    fn drop(&mut self) {
        // SAFETY: each guard owns exactly one successful IP Helper allocation.
        unsafe {
            FreeMibTable(self.0.cast());
        }
    }
}

/// Reads one physical adapter's OS counters, independent of `BiFlow` routing.
///
/// # Errors
/// Returns a stable classification if an IP Helper call fails. No raw OS text,
/// names, GUIDs, addresses, MACs or route destinations leave this boundary.
pub fn sample() -> Result<Option<Sample>, &'static str> {
    // SAFETY: successful OS allocations are owned by RAII guards. Flexible
    // arrays are read from their actual aligned field, bounded by NumEntries.
    // APIs are read-only; no interface or route is changed.
    unsafe { read() }
}

unsafe fn read() -> Result<Option<Sample>, &'static str> {
    let mut interfaces: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    GetIfTable2(&raw mut interfaces)
        .ok()
        .map_err(|_| "interface_table_failed")?;
    let interfaces = Table(interfaces);
    let mut routes: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    GetIpForwardTable2(AF_UNSPEC, &raw mut routes)
        .ok()
        .map_err(|_| "route_table_failed")?;
    let routes = Table(routes);
    let rows = std::slice::from_raw_parts(
        std::ptr::addr_of!((*interfaces.0).Table).cast(),
        (*interfaces.0).NumEntries as usize,
    );
    let route_rows = std::slice::from_raw_parts(
        std::ptr::addr_of!((*routes.0).Table).cast(),
        (*routes.0).NumEntries as usize,
    );
    let mut candidates = Vec::new();
    for row in rows {
        let row: &::windows::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2 = row;
        let class = match row.Type {
            6 => "ethernet",
            71 => "wifi",
            _ => continue,
        };
        let hardware = row.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
        if !hardware || row.OperStatus != IfOperStatusUp {
            continue;
        }
        let mut metric = None;
        for route in route_rows {
            let route: &::windows::Win32::NetworkManagement::IpHelper::MIB_IPFORWARD_ROW2 = route;
            if route.InterfaceIndex != row.InterfaceIndex
                || route.DestinationPrefix.PrefixLength != 0
                || route.Loopback
            {
                continue;
            }
            let mut interface = MIB_IPINTERFACE_ROW::default();
            InitializeIpInterfaceEntry(&raw mut interface);
            interface.Family = route.DestinationPrefix.Prefix.si_family;
            interface.InterfaceLuid = row.InterfaceLuid;
            GetIpInterfaceEntry(&raw mut interface)
                .ok()
                .map_err(|_| "interface_metric_failed")?;
            let cost = u64::from(route.Metric) + u64::from(interface.Metric);
            metric = Some(metric.map_or(cost, |before: u64| before.min(cost)));
        }
        candidates.push(Candidate {
            sample: Sample {
                identity: row.InterfaceLuid.Value,
                class,
                sent: row.OutOctets,
                received: row.InOctets,
            },
            hardware,
            up: true,
            default_metric: metric,
        });
    }
    Ok(select(&candidates))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires an active physical Wi-Fi/Ethernet adapter; run on the connected Windows host"]
    fn active_host_measures_physical_throughput_without_a_routing_engine() {
        let mut tracker = crate::RateTracker::default();
        let before = super::sample()
            .expect("first OS snapshot")
            .expect("active physical adapter");
        assert_eq!(
            tracker.update(Some(before), std::time::Instant::now()),
            None
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        let after = super::sample()
            .expect("second OS snapshot")
            .expect("active physical adapter");
        assert_eq!(before.identity, after.identity);
        assert!(matches!(after.class, "wifi" | "ethernet"));
        let rates = tracker
            .update(Some(after), std::time::Instant::now())
            .expect("physical counter delta");
        assert!(rates.upload.is_finite() && rates.download.is_finite());
        println!(
            "Physical adapter class: {}; send: {:.0} bps; receive: {:.0} bps",
            after.class,
            rates.upload * 8.0,
            rates.download * 8.0
        );
    }

    #[test]
    fn host_ip_helper_snapshot_is_readable_without_elevation() {
        let result = super::sample().expect("IP Helper host snapshot");
        if let Some(sample) = result {
            assert!(matches!(sample.class, "wifi" | "ethernet"));
        }
    }
}

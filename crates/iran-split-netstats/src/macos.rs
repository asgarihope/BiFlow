//! Physical adapter counters for the menu bar.
//!
//! `NET_RT_IFLIST2`'s `ifi_ibytes` does not match the 64-bit link total on
//! current Darwin. `IFMIB_IFDATA` / `IFDATA_GENERAL` does, and it is what
//! `netstat -ib` reports. Names, addresses, and routes stay inside this module.
use crate::{select, Candidate, Sample};
use std::mem::size_of;

const CTL_NET: i32 = 4;
const PF_LINK: i32 = 18;
const NETLINK_GENERIC: i32 = 0;
const IFMIB_SYSTEM: i32 = 1;
const IFMIB_IFDATA: i32 = 2;
const IFMIB_IFCOUNT: i32 = 1;
const IFDATA_GENERAL: i32 = 1;
const NET_RT_DUMP2: i32 = 7;
const ROUTE_HEADER: usize = 92;
const RTF_UP: u32 = 0x1;
const RTF_HOST: u32 = 0x4;
const RTF_REJECT: u32 = 0x8;
const RTF_BLACKHOLE: u32 = 0x1000;
const RTF_GLOBAL: u32 = 0x4000_0000;
const IFF_UP: u32 = 0x1;
const IFF_LOOPBACK: u32 = 0x8;
const IFF_POINTOPOINT: u32 = 0x10;
const IFF_RUNNING: u32 = 0x40;
const IFM_NMASK: i32 = 0x0000_00e0;
const IFM_ETHER: i32 = 0x0000_0020;
const IFM_IEEE80211: i32 = 0x0000_0080;
const IFM_AVALID: i32 = 0x1;
const IFM_ACTIVE: i32 = 0x2;
/// `_IOWR('i', 56, struct ifmediareq)` for the 44-byte media request.
const SIOCGIFMEDIA: libc::c_ulong = 0xc02c_6938;

#[repr(C, packed(4))]
struct IfData64 {
    kind: u8,
    _unused: [u8; 7],
    _mtu_metric: [u32; 2],
    _before_bytes: [u64; 6],
    received: u64,
    sent: u64,
    _tail: [u8; 48],
}

#[repr(C)]
struct IfMibData {
    name: [u8; 16],
    _pcount: u32,
    flags: u32,
    _queue: [u32; 3],
    _filler: [u32; 4],
    data: IfData64,
}

#[repr(C, packed(4))]
struct IfMediaReq {
    name: [u8; 16],
    current: i32,
    _mask: i32,
    status: i32,
    _active: i32,
    _count: i32,
    _list: u64,
}

struct Adapter {
    index: u16,
    class: &'static str,
    up: bool,
    sent: u64,
    received: u64,
}

const _: () = assert!(size_of::<IfData64>() == 128);
const _: () = assert!(size_of::<IfMibData>() == 180);
const _: () = assert!(size_of::<IfMediaReq>() == 44);

/// Reads one active hardware adapter without privileges or network changes.
///
/// # Errors
/// Returns only safe classifications. Interface names and addresses stay private.
pub fn sample() -> Result<Option<Sample>, &'static str> {
    let probe = MediaProbe::open();
    let adapters = read_adapters(&probe)?;
    let defaults = read_default_metrics()?;
    Ok(choose(&adapters, &defaults))
}

fn read_adapters(probe: &MediaProbe) -> Result<Vec<Adapter>, &'static str> {
    let count = interface_count()?;
    let mut adapters = Vec::new();
    for index in 1..=count {
        if let Some(adapter) = read_adapter(index, probe)? {
            adapters.push(adapter);
        }
    }
    Ok(adapters)
}

fn interface_count() -> Result<i32, &'static str> {
    let mut mib = [
        CTL_NET,
        PF_LINK,
        NETLINK_GENERIC,
        IFMIB_SYSTEM,
        IFMIB_IFCOUNT,
    ];
    let mut count = 0i32;
    let mut len = size_of::<i32>();
    sysctl(&mut mib, (&raw mut count).cast(), &raw mut len)?;
    if count < 0 {
        return Err("interface_table_failed");
    }
    Ok(count)
}

fn read_adapter(index: i32, probe: &MediaProbe) -> Result<Option<Adapter>, &'static str> {
    let mut mib = [
        CTL_NET,
        PF_LINK,
        NETLINK_GENERIC,
        IFMIB_IFDATA,
        index,
        IFDATA_GENERAL,
    ];
    let mut row = zero_mib();
    let mut len = size_of::<IfMibData>();
    if let Err(error) = sysctl(&mut mib, (&raw mut row).cast(), &raw mut len) {
        if error == "interface_missing" {
            return Ok(None);
        }
        return Err(error);
    }
    if len < size_of::<IfMibData>() {
        return Err("interface_table_failed");
    }
    let Some(name) = c_name(&row.name) else {
        return Ok(None);
    };
    if is_virtual(name) || row.flags & (IFF_LOOPBACK | IFF_POINTOPOINT) != 0 {
        return Ok(None);
    }
    let media = probe.media(name);
    let Some(class) = media
        .as_ref()
        .map(|media| media.current)
        .and_then(media_class)
        .or_else(|| type_class(row.data.kind))
    else {
        return Ok(None);
    };
    let up = link_up(row.flags, media.map(|media| media.status));
    let index = u16::try_from(index).map_err(|_| "interface_table_failed")?;
    Ok(Some(Adapter {
        index,
        class,
        up,
        sent: row.data.sent,
        received: row.data.received,
    }))
}

fn read_default_metrics() -> Result<Vec<(u16, u64)>, &'static str> {
    let mut metrics = Vec::new();
    for family in [libc::AF_INET, libc::AF_INET6] {
        let mut mib = [CTL_NET, libc::PF_ROUTE, 0, family, NET_RT_DUMP2, 0];
        let table = sysctl_vec(&mut mib)?;
        for (index, metric) in parse_default_metrics(&table)? {
            remember(&mut metrics, index, metric);
        }
    }
    Ok(metrics)
}

fn choose(adapters: &[Adapter], defaults: &[(u16, u64)]) -> Option<Sample> {
    let candidates: Vec<Candidate> = adapters
        .iter()
        .filter(|adapter| adapter.up)
        .map(|adapter| Candidate {
            sample: Sample {
                identity: u64::from(adapter.index),
                class: adapter.class,
                sent: adapter.sent,
                received: adapter.received,
            },
            hardware: true,
            up: true,
            default_metric: defaults
                .iter()
                .find(|(index, _)| *index == adapter.index)
                .map(|(_, metric)| *metric),
        })
        .collect();
    select(&candidates)
}

fn remember(metrics: &mut Vec<(u16, u64)>, index: u16, metric: u64) {
    if let Some(existing) = metrics.iter_mut().find(|(id, _)| *id == index) {
        existing.1 = existing.1.min(metric);
    } else {
        metrics.push((index, metric));
    }
}

fn parse_default_metrics(table: &[u8]) -> Result<Vec<(u16, u64)>, &'static str> {
    let mut metrics = Vec::new();
    let mut offset = 0;
    while offset + 4 <= table.len() {
        let msglen = usize::from(u16::from_ne_bytes([table[offset], table[offset + 1]]));
        if msglen < 4 || offset.saturating_add(msglen) > table.len() {
            return Err("route_table_invalid");
        }
        if msglen >= ROUTE_HEADER {
            let index = u16::from_ne_bytes([table[offset + 4], table[offset + 5]]);
            let mut flag_bytes = [0u8; 4];
            flag_bytes.copy_from_slice(&table[offset + 8..offset + 12]);
            let flags = u32::from_ne_bytes(flag_bytes);
            let destination = &table[offset + ROUTE_HEADER..offset + msglen];
            if usable_default(flags) && destination_is_default(destination) {
                let metric = if flags & RTF_GLOBAL == 0 {
                    10_000 + u64::from(index)
                } else {
                    0
                };
                remember(&mut metrics, index, metric);
            }
        }
        offset += msglen;
    }
    Ok(metrics)
}

fn usable_default(flags: u32) -> bool {
    flags & RTF_UP != 0 && flags & (RTF_HOST | RTF_REJECT | RTF_BLACKHOLE) == 0
}

fn destination_is_default(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }
    let len = usize::from(bytes[0]);
    if len < 2 || len > bytes.len() {
        return false;
    }
    match bytes[1] {
        2 if len >= 8 => bytes[4..8].iter().all(|byte| *byte == 0),
        30 if len >= 24 => bytes[8..24].iter().all(|byte| *byte == 0),
        _ => false,
    }
}

fn is_virtual(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "lo", "gif", "stf", "anpi", "ap", "awdl", "llw", "bridge", "utun", "ipsec", "ppp", "pktap",
        "vmnet", "vnic", "feth", "tap", "tun", "xhc",
    ];
    PREFIXES.iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn media_class(current: i32) -> Option<&'static str> {
    match current & IFM_NMASK {
        IFM_IEEE80211 => Some("wifi"),
        IFM_ETHER => Some("ethernet"),
        _ => None,
    }
}

fn type_class(kind: u8) -> Option<&'static str> {
    match kind {
        71 => Some("wifi"),
        6 => Some("ethernet"),
        _ => None,
    }
}

fn link_up(flags: u32, media_status: Option<i32>) -> bool {
    if flags & IFF_UP == 0 {
        return false;
    }
    match media_status {
        Some(status) if status & IFM_AVALID != 0 => status & IFM_ACTIVE != 0,
        _ => flags & IFF_RUNNING != 0,
    }
}

fn c_name(bytes: &[u8]) -> Option<&str> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let name = std::str::from_utf8(&bytes[..end]).ok()?;
    (!name.is_empty()).then_some(name)
}

struct Media {
    current: i32,
    status: i32,
}

struct MediaProbe {
    fd: i32,
}

impl MediaProbe {
    fn open() -> Self {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        Self { fd }
    }

    fn media(&self, name: &str) -> Option<Media> {
        if self.fd < 0 {
            return None;
        }
        let mut request = IfMediaReq {
            name: [0; 16],
            current: 0,
            _mask: 0,
            status: 0,
            _active: 0,
            _count: 0,
            _list: 0,
        };
        let bytes = name.as_bytes();
        if bytes.len() >= request.name.len() {
            return None;
        }
        request.name[..bytes.len()].copy_from_slice(bytes);
        let rc = unsafe { libc::ioctl(self.fd, SIOCGIFMEDIA, &raw mut request) };
        if rc < 0 {
            return None;
        }
        Some(Media {
            current: request.current,
            status: request.status,
        })
    }
}

impl Drop for MediaProbe {
    fn drop(&mut self) {
        if self.fd < 0 {
            return;
        }
        // `Drop` cannot report the error. Retry an interrupted close, then
        // abandon the descriptor; the next sample opens a new socket.
        while unsafe { libc::close(self.fd) } < 0 {
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                break;
            }
        }
    }
}

fn zero_mib() -> IfMibData {
    IfMibData {
        name: [0; 16],
        _pcount: 0,
        flags: 0,
        _queue: [0; 3],
        _filler: [0; 4],
        data: IfData64 {
            kind: 0,
            _unused: [0; 7],
            _mtu_metric: [0; 2],
            _before_bytes: [0; 6],
            received: 0,
            sent: 0,
            _tail: [0; 48],
        },
    }
}

fn sysctl(mib: &mut [i32], old: *mut libc::c_void, oldlen: *mut usize) -> Result<(), &'static str> {
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            u32::try_from(mib.len()).unwrap_or(u32::MAX),
            old,
            oldlen,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 {
        Ok(())
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        Err("interface_missing")
    } else {
        Err("interface_table_failed")
    }
}

fn sysctl_vec(mib: &mut [i32]) -> Result<Vec<u8>, &'static str> {
    for _ in 0..2 {
        let mut len = 0usize;
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                u32::try_from(mib.len()).unwrap_or(u32::MAX),
                std::ptr::null_mut(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Err("route_table_failed");
        }
        let mut buffer = vec![0u8; len];
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                u32::try_from(mib.len()).unwrap_or(u32::MAX),
                buffer.as_mut_ptr().cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            buffer.truncate(len);
            return Ok(buffer);
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return Err("route_table_failed");
        }
    }
    Err("route_table_failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sockaddr_v4(addr: [u8; 4]) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = 8;
        bytes[1] = 2;
        bytes[4..].copy_from_slice(&addr);
        bytes
    }

    fn sockaddr_v6(addr: [u8; 16]) -> [u8; 28] {
        let mut bytes = [0u8; 28];
        bytes[0] = 28;
        bytes[1] = 30;
        bytes[8..24].copy_from_slice(&addr);
        bytes
    }

    fn route(index: u16, flags: u32, destination: &[u8]) -> Vec<u8> {
        let msglen = ROUTE_HEADER + destination.len();
        let mut bytes = vec![0u8; msglen];
        bytes[..2].copy_from_slice(&u16::try_from(msglen).unwrap().to_ne_bytes());
        bytes[4..6].copy_from_slice(&index.to_ne_bytes());
        bytes[8..12].copy_from_slice(&flags.to_ne_bytes());
        bytes[ROUTE_HEADER..].copy_from_slice(destination);
        bytes
    }

    fn adapter(index: u16, class: &'static str, up: bool) -> Adapter {
        Adapter {
            index,
            class,
            up,
            sent: 14_000,
            received: 13_000,
        }
    }

    #[test]
    fn virtual_names_and_media_separate_wifi_from_tunnels() {
        assert!(is_virtual("utun9"));
        assert!(is_virtual("awdl0"));
        assert!(is_virtual("bridge0"));
        assert!(is_virtual("anpi1"));
        assert!(is_virtual("ap1"));
        assert!(!is_virtual("en0"));
        assert!(!is_virtual("en10"));
        assert_eq!(media_class(0x80), Some("wifi"));
        assert_eq!(media_class(0x0010_0020), Some("ethernet"));
        assert_eq!(media_class(0x2), None);
        assert!(link_up(0x8863, Some(0x3)));
        assert!(!link_up(0x8863, Some(0x1)));
        assert!(!link_up(0x8862, None));
        assert!(link_up(0x8863, None));
    }

    #[test]
    fn global_default_beats_a_tunnel_and_a_host_route() {
        let mut table = route(2, RTF_UP | RTF_GLOBAL, &sockaddr_v4([0, 0, 0, 0]));
        table.extend(route(14, RTF_UP, &sockaddr_v4([0, 0, 0, 0])));
        table.extend(route(
            9,
            RTF_UP | RTF_HOST | RTF_GLOBAL,
            &sockaddr_v4([0, 0, 0, 0]),
        ));
        table.extend(route(3, RTF_UP | RTF_REJECT, &sockaddr_v4([0, 0, 0, 0])));
        table.extend(route(
            4,
            RTF_UP | RTF_GLOBAL,
            &sockaddr_v4([192, 168, 1, 1]),
        ));
        let mut v6 = [0u8; 16];
        v6[15] = 1;
        table.extend(route(5, RTF_UP | RTF_GLOBAL, &sockaddr_v6(v6)));
        table.extend(route(7, RTF_UP | RTF_GLOBAL, &sockaddr_v6([0; 16])));
        let metrics = parse_default_metrics(&table).unwrap();
        assert_eq!(metrics, [(2, 0), (14, 10_014), (7, 0)]);
        assert_eq!(
            parse_default_metrics(&[1, 0, 0, 0]),
            Err("route_table_invalid")
        );
    }

    #[test]
    fn active_link_with_the_default_route_is_the_only_sample() {
        let adapters = [
            adapter(4, "ethernet", false),
            adapter(14, "wifi", true),
            adapter(8, "ethernet", true),
        ];
        let chosen = choose(&adapters, &[(14, 0), (8, 10_008)]).unwrap();
        assert_eq!(chosen.identity, 14);
        assert_eq!(chosen.class, "wifi");
        assert_eq!(chosen.sent, 14_000);
        assert_eq!(choose(&adapters, &[]), None);
        assert_eq!(
            choose(&[adapter(14, "wifi", true)], &[]).unwrap().identity,
            14
        );
    }

    #[test]
    fn host_snapshot_is_readable_without_elevation() {
        let result = sample().expect("macOS physical interface snapshot");
        if let Some(sample) = result {
            assert!(matches!(sample.class, "wifi" | "ethernet"));
        }
    }
}

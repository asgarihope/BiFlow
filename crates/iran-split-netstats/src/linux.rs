use crate::{select, Candidate, Sample};
use std::{fs, io, path::Path};

/// Reads a single active hardware adapter without privileges or network changes.
///
/// # Errors
/// Returns only safe classifications; interface names and addresses stay private.
pub fn sample() -> Result<Option<Sample>, &'static str> {
    read(Path::new("/sys/class/net"), Path::new("/proc/net"))
}

fn read(sys: &Path, proc: &Path) -> Result<Option<Sample>, &'static str> {
    let ipv4 = fs::read_to_string(proc.join("route")).map_err(|_| "route_table_failed")?;
    let ipv6 = match fs::read_to_string(proc.join("ipv6_route")) {
        Ok(table) => table,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err("route_table_failed"),
    };
    let mut defaults = default_routes(&ipv4, false)?;
    defaults.extend(default_routes(&ipv6, true)?);
    let mut candidates = Vec::new();
    for entry in fs::read_dir(sys).map_err(|_| "interface_table_failed")? {
        let entry = entry.map_err(|_| "interface_table_failed")?;
        let path = entry.path();
        // Virtual Ethernet, bridges and TUNs have no hardware device link.
        if !path.join("device").exists() {
            continue;
        }
        if number(&path.join("type"))? != 1
            || fs::read_to_string(path.join("operstate"))
                .map_err(|_| "interface_state_failed")?
                .trim()
                != "up"
        {
            continue;
        }
        let name = entry.file_name();
        let metric = defaults
            .iter()
            .filter(|(interface, _)| Some(*interface) == name.to_str())
            .map(|(_, metric)| *metric)
            .min();
        candidates.push(Candidate {
            sample: Sample {
                identity: number(&path.join("ifindex"))?,
                class: if path.join("wireless").exists() {
                    "wifi"
                } else {
                    "ethernet"
                },
                sent: number(&path.join("statistics/tx_bytes"))?,
                received: number(&path.join("statistics/rx_bytes"))?,
            },
            hardware: true,
            up: true,
            default_metric: metric,
        });
    }
    Ok(select(&candidates))
}

fn number(path: &Path) -> Result<u64, &'static str> {
    fs::read_to_string(path)
        .map_err(|_| "interface_counter_failed")?
        .trim()
        .parse()
        .map_err(|_| "interface_counter_invalid")
}

fn default_routes(table: &str, ipv6: bool) -> Result<Vec<(&str, u64)>, &'static str> {
    let mut defaults = Vec::new();
    for line in table.lines().skip(usize::from(!ipv6)) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < if ipv6 { 10 } else { 8 } {
            return Err("route_table_invalid");
        }
        let (name, flags, metric) = if ipv6 {
            if fields[0] != "00000000000000000000000000000000" || fields[1] != "00" {
                continue;
            }
            (fields[9], fields[8], u64::from_str_radix(fields[5], 16))
        } else {
            if fields[1] != "00000000" || fields[7] != "00000000" {
                continue;
            }
            (fields[0], fields[3], fields[6].parse())
        };
        let flags = u32::from_str_radix(flags, 16).map_err(|_| "route_table_invalid")?;
        let metric = metric.map_err(|_| "route_table_invalid")?;
        if flags & 1 != 0 && flags & 0x200 == 0 {
            defaults.push((name, metric));
        }
    }
    Ok(defaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(root: &Path, name: &str, id: u64, hardware: bool, up: bool, wifi: bool) {
        let path = root.join(name);
        fs::create_dir_all(path.join("statistics")).unwrap();
        if hardware {
            fs::create_dir(path.join("device")).unwrap();
        }
        if wifi {
            fs::create_dir(path.join("wireless")).unwrap();
        }
        for (file, value) in [
            ("type", "1".to_owned()),
            ("operstate", if up { "up" } else { "down" }.to_owned()),
            ("ifindex", id.to_string()),
            ("statistics/tx_bytes", "14000".to_owned()),
            ("statistics/rx_bytes", "13000".to_owned()),
        ] {
            fs::write(path.join(file), value).unwrap();
        }
    }

    #[test]
    fn physical_counters_exclude_tunnels_bridges_and_down_adapters() {
        let root = tempfile::tempdir().unwrap();
        let sys = root.path().join("net");
        let proc = root.path().join("proc");
        fs::create_dir_all(&proc).unwrap();
        interface(&sys, "vpn", 1, false, true, false);
        interface(&sys, "bridge", 2, false, true, false);
        interface(&sys, "eth", 3, true, false, false);
        interface(&sys, "wifi", 4, true, true, true);
        fs::write(proc.join("route"), "Iface Destination Gateway Flags RefCnt Use Metric Mask\nvpn 00000000 00000000 0001 0 0 0 00000000\nwifi 00000000 00000000 0003 0 0 100 00000000\n").unwrap();
        assert_eq!(
            read(&sys, &proc).unwrap(),
            Some(Sample {
                identity: 4,
                class: "wifi",
                sent: 14000,
                received: 13000
            })
        );
        fs::write(sys.join("wifi/statistics/rx_bytes"), "invalid").unwrap();
        assert_eq!(read(&sys, &proc), Err("interface_counter_invalid"));
        fs::write(sys.join("wifi/operstate"), "down").unwrap();
        assert_eq!(read(&sys, &proc), Ok(None));
    }

    #[test]
    fn ipv6_defaults_use_hex_metrics_and_ignore_reject_routes() {
        let routes = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 00000400 0 0 00000003 wifi\n00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 0 0 00200200 lo\n";
        assert_eq!(default_routes(routes, true).unwrap(), [("wifi", 1024)]);
        assert_eq!(
            default_routes("bad route", true),
            Err("route_table_invalid")
        );
        assert_eq!(
            default_routes("header\neth 00000000 0 invalid 0 0 1 00000000", false),
            Err("route_table_invalid")
        );
    }

    #[test]
    fn physical_default_metrics_and_ipv6_choose_one_interface() {
        let root = tempfile::tempdir().unwrap();
        let sys = root.path().join("net");
        let proc = root.path().join("proc");
        fs::create_dir_all(&proc).unwrap();
        interface(&sys, "eth", 2, true, true, false);
        interface(&sys, "wifi", 3, true, true, true);
        let header = "Iface Destination Gateway Flags RefCnt Use Metric Mask\n";
        fs::write(proc.join("route"), format!("{header}eth 00000000 0 0003 0 0 50 00000000\nwifi 00000000 0 0003 0 0 100 00000000\n")).unwrap();
        assert_eq!(read(&sys, &proc).unwrap().unwrap().identity, 2);
        fs::write(proc.join("route"), header).unwrap();
        assert_eq!(read(&sys, &proc), Ok(None));
        fs::write(proc.join("ipv6_route"), "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 00000010 0 0 00000003 wifi\n").unwrap();
        assert_eq!(read(&sys, &proc).unwrap().unwrap().identity, 3);
        fs::remove_file(proc.join("route")).unwrap();
        assert_eq!(read(&sys, &proc), Err("route_table_failed"));
    }

    #[test]
    fn host_snapshot_is_readable_without_elevation() {
        if let Some(sample) = sample().expect("Linux physical interface snapshot") {
            assert!(matches!(sample.class, "wifi" | "ethernet"));
        }
    }
}

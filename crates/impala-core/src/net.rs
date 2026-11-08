//! Network reachability indicator.
//!
//! "Online" means the kernel has a usable IPv4 default route. That is enough
//! for the status icon and costs nothing (no packets sent). With wlan0 down
//! and only the link-local Ethernet cable present there is no default route,
//! so the app correctly shows "Offline".

use crate::state::NetworkStatus;

const RTF_UP: u32 = 0x0001;

/// Interfaces that carry an active default route, parsed from the text of
/// Linux `/proc/net/route`.
pub fn default_route_interfaces(proc_net_route: &str) -> Vec<String> {
    proc_net_route
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 8 {
                return None;
            }
            let (iface, dest, flags, mask) = (cols[0], cols[1], cols[3], cols[7]);
            let flags = u32::from_str_radix(flags, 16).ok()?;
            (dest == "00000000" && mask == "00000000" && flags & RTF_UP != 0).then(|| iface.to_string())
        })
        .collect()
}

pub fn status_from_route_table(proc_net_route: &str) -> NetworkStatus {
    if default_route_interfaces(proc_net_route).is_empty() {
        NetworkStatus::Offline
    } else {
        NetworkStatus::Online
    }
}

/// Probe the running system. Returns `Unknown` where unsupported.
pub fn probe() -> NetworkStatus {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/net/route") {
            Ok(text) => status_from_route_table(&text),
            Err(_) => NetworkStatus::Unknown,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        NetworkStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n";

    #[test]
    fn wifi_default_route_is_online() {
        let table = format!(
            "{HEADER}wlan0\t00000000\t0104A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
             wlan0\t0004A8C0\t00000000\t0001\t0\t0\t600\t00FCFFFF\t0\t0\t0\n"
        );
        assert_eq!(default_route_interfaces(&table), vec!["wlan0"]);
        assert_eq!(status_from_route_table(&table), NetworkStatus::Online);
    }

    #[test]
    fn link_local_only_is_offline() {
        let table = format!("{HEADER}eth0\t0000FEA9\t00000000\t0001\t0\t0\t1000\t0000FFFF\t0\t0\t0\n");
        assert_eq!(status_from_route_table(&table), NetworkStatus::Offline);
        assert_eq!(status_from_route_table(HEADER), NetworkStatus::Offline);
        assert_eq!(status_from_route_table(""), NetworkStatus::Offline);
    }

    #[test]
    fn down_default_route_is_ignored() {
        let table = format!("{HEADER}wlan0\t00000000\t0104A8C0\t0002\t0\t0\t600\t00000000\t0\t0\t0\n");
        assert_eq!(status_from_route_table(&table), NetworkStatus::Offline);
    }
}

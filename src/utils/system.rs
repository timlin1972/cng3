use sysinfo::Networks;

const TAILSCALE_INTERFACE: &str = "tailscale";
const TAILSCALE_INTERFACE_MAC: &str = "utun";

pub fn get_tailscale_ip() -> Option<String> {
    let networks = Networks::new_with_refreshed_list();
    for (interface_name, network) in &networks {
        if interface_name.starts_with(TAILSCALE_INTERFACE) {
            for ipnetwork in network.ip_networks().iter() {
                // if ipv4
                if ipnetwork.addr.is_ipv4() {
                    return Some(ipnetwork.addr.to_string());
                }
            }
        }
        if interface_name.starts_with(TAILSCALE_INTERFACE_MAC) {
            for ipnetwork in network.ip_networks().iter() {
                // if ipv4
                if let std::net::IpAddr::V4(ip) = ipnetwork.addr {
                    // if the first 1 byte is 100, it's a tailscale ip
                    if ip.octets()[0] == 100 {
                        return Some(ipnetwork.addr.to_string());
                    }
                }
            }
        }
    }

    None
}

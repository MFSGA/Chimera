//! Platform-specific network compatibility helpers.

#[cfg(target_os = "macos")]
pub mod macos {
    use std::net::IpAddr;

    pub fn get_default_network_hardware_port() -> std::io::Result<String> {
        chimera_utils::network::macos::get_default_network_hardware_port()
    }

    pub fn set_dns(service_name: &str, dns: Option<Vec<IpAddr>>) -> std::io::Result<()> {
        chimera_utils::network::macos::set_dns(service_name, dns)
    }

    pub fn get_dns(service_name: &str) -> std::io::Result<Option<Vec<IpAddr>>> {
        chimera_utils::network::macos::get_dns(service_name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use test_log::test;

        #[test]
        fn test_get_default_network_hardware_port() {
            let result = get_default_network_hardware_port();
            println!("{:?}", result);
        }

        #[test]
        fn test_set_dns() {
            let dns_a: IpAddr = "114.114.114.114".parse().unwrap();
            let dns_b: IpAddr = "8.8.8.8".parse().unwrap();
            set_dns("Wi-Fi", Some(vec![dns_a])).unwrap();
            assert_eq!(get_dns("Wi-Fi").unwrap(), Some(vec![dns_a]));
            set_dns("Wi-Fi", Some(vec![dns_a, dns_b])).unwrap();
            assert_eq!(get_dns("Wi-Fi").unwrap(), Some(vec![dns_a, dns_b]));
            set_dns("Wi-Fi", None).unwrap();
            assert!(get_dns("Wi-FI").unwrap().is_none());
        }

        #[test]
        fn test_get_dns() {
            let result = get_dns("Wi-Fi").unwrap();
            println!("{:?}", result);
        }
    }
}

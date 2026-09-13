#[cfg(target_os = "windows")]
mod platform {
    use std::{
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
        ptr, slice,
    };

    use windows_sys::Win32::{
        Foundation::ERROR_BUFFER_OVERFLOW,
        NetworkManagement::IpHelper::{
            FreeMibTable, GetAdaptersAddresses, GetIpForwardTable2, IP_ADAPTER_ADDRESSES_LH,
            MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2,
        },
        Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC},
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct AdapterIdentity {
        ipv4_index: u32,
        ipv6_index: u32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ExpectedRoute {
        address: IpAddr,
        prefix_len: u8,
    }

    impl ExpectedRoute {
        fn parse(value: &str) -> Option<Self> {
            let (address, prefix_len) = value.trim().split_once('/')?;
            let address = address.parse::<IpAddr>().ok()?;
            let prefix_len = prefix_len.parse::<u8>().ok()?;
            Self::new(address, prefix_len)
        }

        fn new(address: IpAddr, prefix_len: u8) -> Option<Self> {
            let address = match address {
                IpAddr::V4(address) if prefix_len <= 32 => {
                    let raw = u32::from_be_bytes(address.octets());
                    let mask = if prefix_len == 0 {
                        0
                    } else {
                        u32::MAX << (32 - prefix_len)
                    };
                    IpAddr::V4(Ipv4Addr::from((raw & mask).to_be_bytes()))
                }
                IpAddr::V6(address) if prefix_len <= 128 => {
                    let raw = u128::from_be_bytes(address.octets());
                    let mask = if prefix_len == 0 {
                        0
                    } else {
                        u128::MAX << (128 - prefix_len)
                    };
                    IpAddr::V6(Ipv6Addr::from((raw & mask).to_be_bytes()))
                }
                _ => return None,
            };
            Some(Self {
                address,
                prefix_len,
            })
        }
    }

    pub(super) fn active(
        device: &str,
        auto_route: Option<bool>,
        route_addresses: &[String],
    ) -> Option<bool> {
        let adapter = match find_adapter(device)? {
            Some(adapter) => adapter,
            None => return Some(false),
        };
        match auto_route {
            Some(false) => Some(true),
            Some(true) => route_present(adapter, route_addresses),
            None => None,
        }
    }

    #[cfg(test)]
    pub(super) fn adapter_present(device: &str) -> Option<bool> {
        find_adapter(device).map(|adapter| adapter.is_some())
    }

    fn find_adapter(device: &str) -> Option<Option<AdapterIdentity>> {
        let expected = device.trim();
        if expected.is_empty() {
            return None;
        }

        let mut size = 0_u32;
        let first = unsafe {
            GetAdaptersAddresses(AF_UNSPEC as u32, 0, ptr::null(), ptr::null_mut(), &mut size)
        };
        if first != ERROR_BUFFER_OVERFLOW || size == 0 {
            return None;
        }

        let mut buffer = vec![0_u8; size as usize];
        let addresses = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let result =
            unsafe { GetAdaptersAddresses(AF_UNSPEC as u32, 0, ptr::null(), addresses, &mut size) };
        if result != 0 {
            return None;
        }

        let mut current = addresses;
        while !current.is_null() {
            let friendly_name = unsafe { wide_string((*current).FriendlyName) };
            if friendly_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(expected))
            {
                let ipv4_index = unsafe { (*current).Anonymous1.Anonymous.IfIndex };
                let ipv6_index = unsafe { (*current).Ipv6IfIndex };
                return Some(Some(AdapterIdentity {
                    ipv4_index,
                    ipv6_index,
                }));
            }
            current = unsafe { (*current).Next };
        }
        Some(None)
    }

    fn route_present(adapter: AdapterIdentity, route_addresses: &[String]) -> Option<bool> {
        let expected = if route_addresses.is_empty() {
            Vec::new()
        } else {
            route_addresses
                .iter()
                .map(|route| ExpectedRoute::parse(route))
                .collect::<Option<Vec<_>>>()?
        };

        let mut table = ptr::null_mut::<MIB_IPFORWARD_TABLE2>();
        let result = unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) };
        if result != 0 || table.is_null() {
            return None;
        }

        let route_present = unsafe {
            let rows = slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            let matching = rows
                .iter()
                .filter(|row| route_matches_adapter(row, adapter));
            let observed = matching.filter_map(route_prefix).collect::<Vec<_>>();
            if expected.is_empty() {
                default_capture_routes_present(&observed)
            } else {
                expected.iter().all(|route| observed.contains(route))
            }
        };
        unsafe { FreeMibTable(table.cast()) };
        Some(route_present)
    }

    fn route_matches_adapter(row: &MIB_IPFORWARD_ROW2, adapter: AdapterIdentity) -> bool {
        (adapter.ipv4_index != 0 && row.InterfaceIndex == adapter.ipv4_index)
            || (adapter.ipv6_index != 0 && row.InterfaceIndex == adapter.ipv6_index)
    }

    fn default_capture_routes_present(routes: &[ExpectedRoute]) -> bool {
        let ipv4_default = ExpectedRoute::parse("0.0.0.0/0").expect("static IPv4 route");
        let ipv4_low = ExpectedRoute::parse("0.0.0.0/1").expect("static IPv4 route");
        let ipv4_high = ExpectedRoute::parse("128.0.0.0/1").expect("static IPv4 route");
        let ipv6_default = ExpectedRoute::parse("::/0").expect("static IPv6 route");
        let ipv6_low = ExpectedRoute::parse("::/1").expect("static IPv6 route");
        let ipv6_high = ExpectedRoute::parse("8000::/1").expect("static IPv6 route");

        routes.contains(&ipv4_default)
            || (routes.contains(&ipv4_low) && routes.contains(&ipv4_high))
            || routes.contains(&ipv6_default)
            || (routes.contains(&ipv6_low) && routes.contains(&ipv6_high))
    }

    fn route_prefix(row: &MIB_IPFORWARD_ROW2) -> Option<ExpectedRoute> {
        let prefix = &row.DestinationPrefix;
        let family = unsafe { prefix.Prefix.si_family };
        if family == AF_INET {
            let bytes = unsafe { prefix.Prefix.Ipv4.sin_addr.S_un.S_un_b };
            ExpectedRoute::new(
                IpAddr::V4(Ipv4Addr::new(
                    bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4,
                )),
                prefix.PrefixLength,
            )
        } else if family == AF_INET6 {
            let bytes = unsafe { prefix.Prefix.Ipv6.sin6_addr.u.Byte };
            ExpectedRoute::new(IpAddr::V6(Ipv6Addr::from(bytes)), prefix.PrefixLength)
        } else {
            None
        }
    }

    unsafe fn wide_string(ptr: *const u16) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let mut len = 0_usize;
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        Some(String::from_utf16_lossy(unsafe {
            slice::from_raw_parts(ptr, len)
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::{
            AF_UNSPEC, ExpectedRoute, FreeMibTable, GetIpForwardTable2, MIB_IPFORWARD_TABLE2,
            default_capture_routes_present,
        };
        use std::{
            net::{IpAddr, Ipv4Addr, Ipv6Addr},
            ptr,
        };

        #[test]
        fn expected_routes_are_canonicalized_before_comparison() {
            assert_eq!(
                ExpectedRoute::parse("198.18.1.9/16"),
                Some(ExpectedRoute {
                    address: IpAddr::V4(Ipv4Addr::new(198, 18, 0, 0)),
                    prefix_len: 16,
                })
            );
            assert_eq!(
                ExpectedRoute::parse("8000::1234/1"),
                Some(ExpectedRoute {
                    address: IpAddr::V6(Ipv6Addr::new(0x8000, 0, 0, 0, 0, 0, 0, 0)),
                    prefix_len: 1,
                })
            );
            assert_eq!(ExpectedRoute::parse("0.0.0.0/33"), None);
        }

        #[test]
        fn default_capture_requires_a_complete_route_set() {
            let low = ExpectedRoute::parse("0.0.0.0/1").unwrap();
            let high = ExpectedRoute::parse("128.0.0.0/1").unwrap();
            assert!(!default_capture_routes_present(&[low]));
            assert!(default_capture_routes_present(&[low, high]));

            let default_v4 = ExpectedRoute::parse("0.0.0.0/0").unwrap();
            assert!(default_capture_routes_present(&[default_v4]));
        }

        #[test]
        fn windows_route_table_is_readable() {
            let mut table = ptr::null_mut::<MIB_IPFORWARD_TABLE2>();
            let result = unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) };
            assert_eq!(result, 0);
            assert!(!table.is_null());
            unsafe { FreeMibTable(table.cast()) };
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn active(
    device: Option<&str>,
    auto_route: Option<bool>,
    route_addresses: &[String],
) -> Option<bool> {
    device.and_then(|device| platform::active(device, auto_route, route_addresses))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn active(
    _device: Option<&str>,
    _auto_route: Option<bool>,
    _route_addresses: &[String],
) -> Option<bool> {
    None
}

#[cfg(all(test, target_os = "windows"))]
pub(super) fn adapter_present(device: Option<&str>) -> Option<bool> {
    device.and_then(platform::adapter_present)
}

#[cfg(all(test, not(target_os = "windows")))]
pub(super) fn adapter_present(_device: Option<&str>) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_or_missing_device_is_never_claimed_active() {
        assert_eq!(super::adapter_present(None), None);
        assert_eq!(super::adapter_present(Some("")), None);
        assert_eq!(super::adapter_present(Some("   ")), None);
        assert_eq!(super::active(None, Some(true), &[]), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_adapter_enumeration_can_prove_a_missing_device() {
        let device = "__chimera_agent_test_missing_tun_device__";
        assert_eq!(super::adapter_present(Some(device)), Some(false));
        assert_eq!(super::active(Some(device), Some(true), &[]), Some(false));
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{ptr, slice};

    use windows_sys::Win32::{
        Foundation::ERROR_BUFFER_OVERFLOW,
        NetworkManagement::IpHelper::{GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH},
        Networking::WinSock::AF_UNSPEC,
    };

    pub(super) fn adapter_present(device: &str) -> Option<bool> {
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
                return Some(true);
            }
            current = unsafe { (*current).Next };
        }
        Some(false)
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
}

#[cfg(target_os = "windows")]
pub(super) fn adapter_present(device: Option<&str>) -> Option<bool> {
    device.and_then(platform::adapter_present)
}

#[cfg(not(target_os = "windows"))]
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
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_adapter_enumeration_can_prove_a_missing_device() {
        assert_eq!(
            super::adapter_present(Some("__chimera_agent_test_missing_tun_device__")),
            Some(false)
        );
    }
}

use std::{
    mem::size_of,
    net::{IpAddr, SocketAddr},
    ptr::null,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use reqwest::redirect::Policy;
use tokio::{net::TcpStream, sync::Semaphore};
use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST,
            GetAdaptersAddresses, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211,
            IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, IpDadStatePreferred},
};

use super::super::{
    host_connectivity::{HostConnectivityEvidence, diagnose_host_connectivity},
    model::AgentHostConnectivitySnapshot,
    ports::HostConnectivityPort,
};

const HOST_CONNECTIVITY_TIMEOUT: Duration = Duration::from_millis(1_800);
const SUCCESS_CACHE_TTL: Duration = Duration::from_secs(10);
const FAILURE_CACHE_TTL: Duration = Duration::from_secs(2);
const DNS_LOOKUP_TIMEOUT: Duration = Duration::from_millis(500);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const CAPTIVE_PORTAL_TIMEOUT: Duration = Duration::from_millis(700);
const MAX_ADAPTER_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_ADAPTERS: usize = 256;
const MAX_ADDRESSES_PER_ADAPTER: usize = 256;
const MAX_RESOLVED_ADDRESSES: usize = 16;
const CONNECTIVITY_HOST: &str = "www.msftconnecttest.com";
const CONNECTIVITY_PORT: u16 = 80;
const CONNECTIVITY_URL: &str = "http://www.msftconnecttest.com/connecttest.txt";
const CONNECTIVITY_BODY: &[u8] = b"Microsoft Connect Test";

#[derive(Clone)]
struct CachedConnectivity {
    snapshot: AgentHostConnectivitySnapshot,
    expires_at: Instant,
}

#[async_trait::async_trait]
trait ConnectivityCollector: Send + Sync {
    async fn collect(&self) -> AgentHostConnectivitySnapshot;
}

struct NativeConnectivityCollector;

#[async_trait::async_trait]
impl ConnectivityCollector for NativeConnectivityCollector {
    async fn collect(&self) -> AgentHostConnectivitySnapshot {
        collect_connectivity_snapshot().await
    }
}

pub(crate) struct WindowsHostConnectivity {
    single_flight: Arc<Semaphore>,
    cache: Arc<Mutex<Option<CachedConnectivity>>>,
    collector: Arc<dyn ConnectivityCollector>,
    timeout: Duration,
}

impl WindowsHostConnectivity {
    pub(crate) fn new() -> Self {
        Self::with_collector(
            Arc::new(NativeConnectivityCollector),
            HOST_CONNECTIVITY_TIMEOUT,
        )
    }

    fn with_collector(collector: Arc<dyn ConnectivityCollector>, timeout: Duration) -> Self {
        Self {
            single_flight: Arc::new(Semaphore::new(1)),
            cache: Arc::new(Mutex::new(None)),
            collector,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl HostConnectivityPort for WindowsHostConnectivity {
    async fn snapshot(&self) -> AgentHostConnectivitySnapshot {
        if let Some(snapshot) = cached_snapshot(&self.cache, Instant::now()) {
            return snapshot;
        }

        let single_flight = self.single_flight.clone();
        let cache = self.cache.clone();
        let collector = self.collector.clone();
        let timeout = self.timeout;
        let task = tokio::spawn(async move {
            let _permit = match single_flight.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return unavailable_snapshot(),
            };
            if let Some(snapshot) = cached_snapshot(&cache, Instant::now()) {
                return snapshot;
            }
            let snapshot = collector.collect().await;
            store_cached_snapshot(&cache, snapshot.clone(), Instant::now());
            snapshot
        });
        match tokio::time::timeout(timeout, task).await {
            Ok(Ok(snapshot)) => snapshot,
            _ => unavailable_snapshot(),
        }
    }
}

async fn collect_connectivity_snapshot() -> AgentHostConnectivitySnapshot {
    let Some(evidence) = tokio::task::spawn_blocking(collect_windows_evidence)
        .await
        .ok()
        .flatten()
    else {
        return unavailable_snapshot();
    };
    diagnose_host_connectivity(enrich_external_evidence(evidence).await)
}

fn cached_snapshot(
    cache: &Mutex<Option<CachedConnectivity>>,
    now: Instant,
) -> Option<AgentHostConnectivitySnapshot> {
    let guard = cache.lock().ok()?;
    guard
        .as_ref()
        .filter(|entry| entry.expires_at > now)
        .map(|entry| entry.snapshot.clone())
}

fn store_cached_snapshot(
    cache: &Mutex<Option<CachedConnectivity>>,
    snapshot: AgentHostConnectivitySnapshot,
    now: Instant,
) {
    let ttl = if snapshot.status == super::super::model::AgentHostConnectivityStatus::Indeterminate
    {
        FAILURE_CACHE_TTL
    } else {
        SUCCESS_CACHE_TTL
    };
    if let Ok(mut guard) = cache.lock() {
        *guard = Some(CachedConnectivity {
            snapshot,
            expires_at: now + ttl,
        });
    }
}

fn unavailable_snapshot() -> AgentHostConnectivitySnapshot {
    diagnose_host_connectivity(HostConnectivityEvidence::default())
}

async fn enrich_external_evidence(
    mut evidence: HostConnectivityEvidence,
) -> HostConnectivityEvidence {
    if evidence.dns_configured != Some(true) {
        return evidence;
    }
    let Some(addresses) = resolve_connectivity_addresses().await else {
        evidence.dns_resolves = Some(false);
        return evidence;
    };
    evidence.dns_resolves = Some(true);
    let (ipv4, ipv6) = tokio::join!(
        connect_family(&addresses, false, evidence.ipv4_usable_address),
        connect_family(&addresses, true, evidence.ipv6_usable_address),
    );
    evidence.ipv4_internet_reachable = ipv4;
    evidence.ipv6_internet_reachable = ipv6;
    evidence.captive_portal_suspected = probe_captive_portal(&addresses).await;
    evidence
}

async fn resolve_connectivity_addresses() -> Option<Vec<SocketAddr>> {
    let resolved = tokio::time::timeout(
        DNS_LOOKUP_TIMEOUT,
        tokio::net::lookup_host((CONNECTIVITY_HOST, CONNECTIVITY_PORT)),
    )
    .await
    .ok()?
    .ok()?;
    let mut addresses = Vec::new();
    for address in resolved.take(MAX_RESOLVED_ADDRESSES) {
        if is_public_probe_ip(address.ip()) && !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    (!addresses.is_empty()).then_some(addresses)
}

async fn connect_family(
    addresses: &[SocketAddr],
    ipv6: bool,
    family_available: bool,
) -> Option<bool> {
    if !family_available {
        return None;
    }
    let candidates = addresses
        .iter()
        .copied()
        .filter(|address| {
            matches!(
                (address.ip(), ipv6),
                (IpAddr::V6(_), true) | (IpAddr::V4(_), false)
            )
        })
        .take(4);
    let mut attempted = false;
    for address in candidates {
        attempted = true;
        if matches!(
            tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(address)).await,
            Ok(Ok(_))
        ) {
            return Some(true);
        }
    }
    attempted.then_some(false)
}

async fn probe_captive_portal(addresses: &[SocketAddr]) -> Option<bool> {
    let address = addresses.first().copied()?;
    let client = reqwest::ClientBuilder::new()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(CAPTIVE_PORTAL_TIMEOUT)
        .resolve(CONNECTIVITY_HOST, address)
        .build()
        .ok()?;
    let response = client.get(CONNECTIVITY_URL).send().await.ok()?;
    if !response.status().is_success() {
        return Some(true);
    }
    let body = response.bytes().await.ok()?;
    Some(body.as_ref() != CONNECTIVITY_BODY)
}

fn is_public_probe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [first, second, ..] = ip.octets();
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_private()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_multicast()
                && !ip.is_documentation()
                && first != 0
                && first < 224
                && !(first == 100 && (64..=127).contains(&second))
                && !(first == 192 && second == 0)
                && !(first == 198 && (second == 18 || second == 19))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && (segments[0] & 0xfe00) != 0xfc00
                && (segments[0] & 0xffc0) != 0xfe80
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

fn collect_windows_evidence() -> Option<HostConnectivityEvidence> {
    let adapters = query_adapters()?;
    let mut evidence = HostConnectivityEvidence {
        probe_complete: true,
        ..HostConnectivityEvidence::default()
    };

    let mut adapter = adapters.head;
    let mut adapter_count = 0;
    while !adapter.is_null() && adapter_count < MAX_ADAPTERS {
        adapter_count += 1;
        // SAFETY: `adapter` points into the owned GetAdaptersAddresses buffer for this scope.
        let current = unsafe { &*adapter };
        if current.IfType != IF_TYPE_SOFTWARE_LOOPBACK
            && current.IfType != IF_TYPE_TUNNEL
            && !project_adapter(current, &mut evidence)
        {
            return None;
        }
        adapter = current.Next;
    }
    (adapter.is_null()).then_some(evidence)
}

fn project_adapter(
    adapter: &IP_ADAPTER_ADDRESSES_LH,
    evidence: &mut HostConnectivityEvidence,
) -> bool {
    let connected = adapter.OperStatus == IfOperStatusUp;
    match adapter.IfType {
        IF_TYPE_IEEE80211 => {
            evidence.wireless_present = true;
            evidence.wireless_connected |= connected;
        }
        IF_TYPE_ETHERNET_CSMACD => {
            evidence.ethernet_present = true;
            evidence.ethernet_connected |= connected;
        }
        _ => evidence.other_interface_connected |= connected,
    }
    if !connected {
        return true;
    }

    if !project_unicast_addresses(adapter, evidence) || !project_gateways(adapter, evidence) {
        return false;
    }
    evidence.dns_configured =
        Some(evidence.dns_configured == Some(true) || !adapter.FirstDnsServerAddress.is_null());
    true
}

fn project_unicast_addresses(
    adapter: &IP_ADAPTER_ADDRESSES_LH,
    evidence: &mut HostConnectivityEvidence,
) -> bool {
    let mut address = adapter.FirstUnicastAddress;
    let mut count = 0;
    while !address.is_null() && count < MAX_ADDRESSES_PER_ADAPTER {
        count += 1;
        // SAFETY: address nodes belong to the same GetAdaptersAddresses buffer.
        let current = unsafe { &*address };
        if current.DadState == IpDadStatePreferred && !current.Address.lpSockaddr.is_null() {
            // SAFETY: lpSockaddr is non-null and points to a SOCKADDR owned by the buffer.
            let family = unsafe { (*current.Address.lpSockaddr).sa_family };
            evidence.ipv4_usable_address |= family == AF_INET;
            evidence.ipv6_usable_address |= family == AF_INET6;
        }
        address = current.Next;
    }
    address.is_null()
}

fn project_gateways(
    adapter: &IP_ADAPTER_ADDRESSES_LH,
    evidence: &mut HostConnectivityEvidence,
) -> bool {
    let mut gateway = adapter.FirstGatewayAddress;
    let mut count = 0;
    while !gateway.is_null() && count < MAX_ADDRESSES_PER_ADAPTER {
        count += 1;
        // SAFETY: gateway nodes belong to the same GetAdaptersAddresses buffer.
        let current = unsafe { &*gateway };
        if !current.Address.lpSockaddr.is_null() {
            // SAFETY: lpSockaddr is non-null and points to a SOCKADDR owned by the buffer.
            let family = unsafe { (*current.Address.lpSockaddr).sa_family };
            evidence.ipv4_default_route |= family == AF_INET;
            evidence.ipv6_default_route |= family == AF_INET6;
        }
        gateway = current.Next;
    }
    gateway.is_null()
}

struct AdapterBuffer {
    _storage: Vec<usize>,
    head: *mut IP_ADAPTER_ADDRESSES_LH,
}

fn query_adapters() -> Option<AdapterBuffer> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut required = 0u32;
    // SAFETY: the initial null-buffer call only requests the required byte size.
    let first = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC as u32,
            flags,
            null(),
            std::ptr::null_mut(),
            &mut required,
        )
    };
    if first != ERROR_BUFFER_OVERFLOW
        || required == 0
        || required as usize > MAX_ADAPTER_BUFFER_BYTES
    {
        return None;
    }

    let words = (required as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    let head = storage.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: storage is pointer-aligned, writable, and at least `required` bytes long.
    let result =
        unsafe { GetAdaptersAddresses(AF_UNSPEC as u32, flags, null(), head, &mut required) };
    if result != NO_ERROR || required as usize > MAX_ADAPTER_BUFFER_BYTES {
        return None;
    }
    Some(AdapterBuffer {
        _storage: storage,
        head,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        net::IpAddr,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use super::{
        CONNECTIVITY_HOST, CONNECTIVITY_URL, ConnectivityCollector, FAILURE_CACHE_TTL,
        MAX_ADAPTER_BUFFER_BYTES, SUCCESS_CACHE_TTL, WindowsHostConnectivity, cached_snapshot,
        is_public_probe_ip, store_cached_snapshot, unavailable_snapshot,
    };
    use crate::features::agent::{
        host_connectivity::{HostConnectivityEvidence, diagnose_host_connectivity},
        model::{AgentHostConnectivitySnapshot, AgentHostConnectivityStatus},
        ports::HostConnectivityPort,
    };

    struct FakeCollector {
        calls: AtomicUsize,
        delay: Duration,
        snapshot: AgentHostConnectivitySnapshot,
    }

    impl FakeCollector {
        fn new(delay: Duration, snapshot: AgentHostConnectivitySnapshot) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                delay,
                snapshot,
            }
        }
    }

    #[async_trait::async_trait]
    impl ConnectivityCollector for FakeCollector {
        async fn collect(&self) -> AgentHostConnectivitySnapshot {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.snapshot.clone()
        }
    }

    fn healthy_ipv4_snapshot() -> AgentHostConnectivitySnapshot {
        diagnose_host_connectivity(HostConnectivityEvidence {
            wireless_present: true,
            wireless_connected: true,
            ipv4_usable_address: true,
            ipv4_default_route: true,
            ipv4_internet_reachable: Some(true),
            dns_configured: Some(true),
            dns_resolves: Some(true),
            captive_portal_suspected: Some(false),
            probe_complete: true,
            ..HostConnectivityEvidence::default()
        })
    }

    #[tokio::test]
    async fn native_probe_returns_only_closed_privacy_safe_state() {
        let snapshot = WindowsHostConnectivity::new().snapshot().await;
        let value = serde_json::to_value(snapshot).expect("serialize connectivity snapshot");
        let text = value.to_string();
        for forbidden in [
            "adapter_name",
            "friendly_name",
            "mac",
            "gateway_address",
            "dns_address",
        ] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn cache_uses_shorter_ttl_for_failed_results_and_refreshes_after_expiry() {
        let cache = Mutex::new(None);
        let now = Instant::now();
        let failed = unavailable_snapshot();
        store_cached_snapshot(&cache, failed.clone(), now);
        assert_eq!(
            cached_snapshot(&cache, now + FAILURE_CACHE_TTL / 2).map(|value| value.status),
            Some(failed.status)
        );
        assert!(cached_snapshot(&cache, now + FAILURE_CACHE_TTL).is_none());

        let healthy = healthy_ipv4_snapshot();
        store_cached_snapshot(&cache, healthy.clone(), now);
        assert_eq!(
            cached_snapshot(&cache, now + Duration::from_secs(5)).map(|value| value.status),
            Some(healthy.status)
        );
        assert!(cached_snapshot(&cache, now + SUCCESS_CACHE_TTL).is_none());
    }

    #[tokio::test]
    async fn concurrent_snapshots_share_one_injected_probe() {
        let collector = Arc::new(FakeCollector::new(
            Duration::from_millis(40),
            healthy_ipv4_snapshot(),
        ));
        let adapter = Arc::new(WindowsHostConnectivity::with_collector(
            collector.clone(),
            Duration::from_millis(250),
        ));

        let (first, second) = tokio::join!(adapter.snapshot(), adapter.snapshot());

        assert_eq!(first.status, AgentHostConnectivityStatus::OnlineIpv4Only);
        assert_eq!(second.status, AgentHostConnectivityStatus::OnlineIpv4Only);
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timed_out_probe_retains_single_flight_and_populates_cache() {
        let collector = Arc::new(FakeCollector::new(
            Duration::from_millis(100),
            healthy_ipv4_snapshot(),
        ));
        let adapter = Arc::new(WindowsHostConnectivity::with_collector(
            collector.clone(),
            Duration::from_millis(20),
        ));
        let first_adapter = adapter.clone();
        let first = tokio::spawn(async move { first_adapter.snapshot().await });
        tokio::time::sleep(Duration::from_millis(5)).await;

        let second = adapter.snapshot().await;
        let first = first.await.expect("join first timed out snapshot");
        assert_eq!(first.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(second.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(110)).await;
        let cached = adapter.snapshot().await;
        assert_eq!(cached.status, AgentHostConnectivityStatus::OnlineIpv4Only);
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn injected_probe_failure_is_cached_as_a_closed_indeterminate_result() {
        let collector = Arc::new(FakeCollector::new(Duration::ZERO, unavailable_snapshot()));
        let adapter =
            WindowsHostConnectivity::with_collector(collector.clone(), Duration::from_millis(100));

        let first = adapter.snapshot().await;
        let second = adapter.snapshot().await;

        assert_eq!(first.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(second.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(first.reasons, second.reasons);
    }

    #[test]
    fn adapter_buffer_budget_is_bounded() {
        assert_eq!(MAX_ADAPTER_BUFFER_BYTES, 1024 * 1024);
    }

    #[test]
    fn connectivity_targets_are_fixed_and_public_only() {
        assert_eq!(CONNECTIVITY_HOST, "www.msftconnecttest.com");
        assert_eq!(
            CONNECTIVITY_URL,
            "http://www.msftconnecttest.com/connecttest.txt"
        );
        for blocked in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
        ] {
            assert!(!is_public_probe_ip(blocked.parse::<IpAddr>().unwrap()));
        }
        for allowed in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(is_public_probe_ip(allowed.parse::<IpAddr>().unwrap()));
        }
    }
}

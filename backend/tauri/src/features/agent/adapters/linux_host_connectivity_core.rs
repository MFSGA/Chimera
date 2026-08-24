use std::{
    collections::BTreeSet,
    mem::size_of,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::sync::Semaphore;

use super::super::{
    host_connectivity::{HostConnectivityEvidence, diagnose_host_connectivity},
    model::{AgentHostConnectivitySnapshot, AgentHostConnectivityStatus},
};

pub(crate) const MAX_INTERFACE_ROWS: usize = 512;
pub(crate) const MAX_ROUTE_MESSAGES: usize = 512;
pub(crate) const MAX_ROUTE_DUMP_BYTES: usize = 128 * 1024;
pub(crate) const ROUTE_DUMP_SEQUENCE: u32 = 0x4348_4d52;

const SUCCESS_CACHE_TTL: Duration = Duration::from_secs(10);
const FAILURE_CACHE_TTL: Duration = Duration::from_secs(2);
const NETLINK_HEADER_LEN: usize = 16;
const ROUTE_MESSAGE_LEN: usize = 12;
const ROUTE_ATTRIBUTE_HEADER_LEN: usize = 4;
const NLMSG_NOOP: u16 = 1;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLMSG_OVERRUN: u16 = 4;
const NLM_F_DUMP_INTR: u16 = 0x0010;
const RTM_NEWROUTE: u16 = 24;
const RTM_GETROUTE: u16 = 26;
const NLM_F_REQUEST: u16 = 0x0001;
const NLM_F_DUMP: u16 = 0x0300;
const AF_UNSPEC: u8 = 0;
const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;
const RTN_UNICAST: u8 = 1;
const RT_TABLE_MAIN: u32 = 254;
const RTA_OIF: u16 = 4;
const RTA_TABLE: u16 = 15;
const NLA_TYPE_MASK: u16 = 0x3fff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SanitizedAddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SanitizedInterfaceRow {
    pub(crate) interface_index: u32,
    pub(crate) up: bool,
    pub(crate) family: Option<SanitizedAddressFamily>,
    pub(crate) usable_address: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SanitizedRouteRow {
    pub(crate) family: SanitizedAddressFamily,
    pub(crate) output_interface: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinuxConnectivityEvidenceError {
    BudgetExceeded,
    Incomplete,
    KernelRejected,
    Malformed,
    #[cfg(target_os = "linux")]
    NativeUnavailable,
}

#[derive(Clone)]
struct CachedConnectivity {
    snapshot: AgentHostConnectivitySnapshot,
    expires_at: Instant,
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

#[async_trait::async_trait]
pub(crate) trait LinuxConnectivityCollector: Send + Sync {
    async fn collect(&self) -> AgentHostConnectivitySnapshot;
}

pub(crate) struct LinuxHostConnectivityCore {
    single_flight: Arc<Semaphore>,
    cache: Arc<Mutex<Option<CachedConnectivity>>>,
    collector: Arc<dyn LinuxConnectivityCollector>,
    timeout: Duration,
    success_ttl: Duration,
    failure_ttl: Duration,
    clock: Clock,
}

impl LinuxHostConnectivityCore {
    pub(crate) fn new(collector: Arc<dyn LinuxConnectivityCollector>, timeout: Duration) -> Self {
        Self::with_policy(
            collector,
            timeout,
            SUCCESS_CACHE_TTL,
            FAILURE_CACHE_TTL,
            Arc::new(Instant::now),
        )
    }

    fn with_policy(
        collector: Arc<dyn LinuxConnectivityCollector>,
        timeout: Duration,
        success_ttl: Duration,
        failure_ttl: Duration,
        clock: Clock,
    ) -> Self {
        Self {
            single_flight: Arc::new(Semaphore::new(1)),
            cache: Arc::new(Mutex::new(None)),
            collector,
            timeout,
            success_ttl,
            failure_ttl,
            clock,
        }
    }

    pub(crate) async fn snapshot(&self) -> AgentHostConnectivitySnapshot {
        let now = (self.clock)();
        if let Some(snapshot) = cached_snapshot(&self.cache, now) {
            return snapshot;
        }

        let single_flight = self.single_flight.clone();
        let cache = self.cache.clone();
        let collector = self.collector.clone();
        let clock = self.clock.clone();
        let success_ttl = self.success_ttl;
        let failure_ttl = self.failure_ttl;
        let task = tokio::spawn(async move {
            let _permit = match single_flight.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return unavailable_snapshot(),
            };
            let now = clock();
            if let Some(snapshot) = cached_snapshot(&cache, now) {
                return snapshot;
            }
            let snapshot = collector.collect().await;
            store_cached_snapshot(&cache, snapshot.clone(), clock(), success_ttl, failure_ttl);
            snapshot
        });

        match tokio::time::timeout(self.timeout, task).await {
            Ok(Ok(snapshot)) => snapshot,
            _ => unavailable_snapshot(),
        }
    }
}

pub(crate) fn merge_linux_evidence(
    interface_rows: &[SanitizedInterfaceRow],
    route_rows: &[SanitizedRouteRow],
) -> Result<HostConnectivityEvidence, LinuxConnectivityEvidenceError> {
    if interface_rows.len() > MAX_INTERFACE_ROWS || route_rows.len() > MAX_ROUTE_MESSAGES {
        return Err(LinuxConnectivityEvidenceError::BudgetExceeded);
    }

    let mut active_interfaces = BTreeSet::new();
    let mut ipv4_usable_address = false;
    let mut ipv6_usable_address = false;
    for row in interface_rows {
        if row.interface_index == 0 {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
        if row.up {
            active_interfaces.insert(row.interface_index);
            match row.family {
                Some(SanitizedAddressFamily::Ipv4) if row.usable_address => {
                    ipv4_usable_address = true;
                }
                Some(SanitizedAddressFamily::Ipv6) if row.usable_address => {
                    ipv6_usable_address = true;
                }
                _ => {}
            }
        }
    }

    let ipv4_default_route = route_rows.iter().any(|route| {
        route.family == SanitizedAddressFamily::Ipv4
            && route.output_interface != 0
            && active_interfaces.contains(&route.output_interface)
    });
    let ipv6_default_route = route_rows.iter().any(|route| {
        route.family == SanitizedAddressFamily::Ipv6
            && route.output_interface != 0
            && active_interfaces.contains(&route.output_interface)
    });

    Ok(HostConnectivityEvidence {
        other_interface_connected: !active_interfaces.is_empty(),
        ipv4_usable_address,
        ipv4_default_route,
        ipv6_usable_address,
        ipv6_default_route,
        probe_complete: true,
        ..HostConnectivityEvidence::default()
    })
}

pub(crate) fn route_dump_request(sequence: u32) -> [u8; NETLINK_HEADER_LEN + ROUTE_MESSAGE_LEN] {
    let mut request = [0_u8; NETLINK_HEADER_LEN + ROUTE_MESSAGE_LEN];
    let request_len = request.len() as u32;
    write_u32(&mut request, 0, request_len);
    write_u16(&mut request, 4, RTM_GETROUTE);
    write_u16(&mut request, 6, NLM_F_REQUEST | NLM_F_DUMP);
    write_u32(&mut request, 8, sequence);
    request[NETLINK_HEADER_LEN] = AF_UNSPEC;
    request
}

pub(crate) struct RouteDumpParser {
    sequence: u32,
    total_bytes: usize,
    message_count: usize,
    done: bool,
    rows: Vec<SanitizedRouteRow>,
}

impl RouteDumpParser {
    pub(crate) fn new(sequence: u32) -> Self {
        Self {
            sequence,
            total_bytes: 0,
            message_count: 0,
            done: false,
            rows: Vec::new(),
        }
    }

    pub(crate) fn push_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Result<bool, LinuxConnectivityEvidenceError> {
        if self.done {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(chunk.len())
            .ok_or(LinuxConnectivityEvidenceError::BudgetExceeded)?;
        if self.total_bytes > MAX_ROUTE_DUMP_BYTES {
            return Err(LinuxConnectivityEvidenceError::BudgetExceeded);
        }

        let mut offset = 0_usize;
        while offset < chunk.len() {
            if chunk.len() - offset < NETLINK_HEADER_LEN {
                return Err(LinuxConnectivityEvidenceError::Malformed);
            }
            let message_len = read_u32(chunk, offset)? as usize;
            if message_len < NETLINK_HEADER_LEN || message_len > chunk.len() - offset {
                return Err(LinuxConnectivityEvidenceError::Malformed);
            }
            self.message_count += 1;
            if self.message_count > MAX_ROUTE_MESSAGES {
                return Err(LinuxConnectivityEvidenceError::BudgetExceeded);
            }

            let message_type = read_u16(chunk, offset + 4)?;
            let message_flags = read_u16(chunk, offset + 6)?;
            let sequence = read_u32(chunk, offset + 8)?;
            if sequence != self.sequence {
                return Err(LinuxConnectivityEvidenceError::Malformed);
            }
            if message_flags & NLM_F_DUMP_INTR != 0 {
                return Err(LinuxConnectivityEvidenceError::Incomplete);
            }
            let payload = &chunk[offset + NETLINK_HEADER_LEN..offset + message_len];
            match message_type {
                NLMSG_NOOP => {}
                NLMSG_ERROR => parse_kernel_error(payload)?,
                NLMSG_DONE => self.done = true,
                NLMSG_OVERRUN => return Err(LinuxConnectivityEvidenceError::Incomplete),
                RTM_NEWROUTE => parse_route_message(payload, &mut self.rows)?,
                _ => {}
            }

            let aligned_len =
                align4(message_len).ok_or(LinuxConnectivityEvidenceError::BudgetExceeded)?;
            let aligned_end = offset
                .checked_add(aligned_len)
                .ok_or(LinuxConnectivityEvidenceError::BudgetExceeded)?;
            if aligned_end <= chunk.len() {
                offset = aligned_end;
            } else if offset + message_len == chunk.len() {
                offset += message_len;
            } else {
                return Err(LinuxConnectivityEvidenceError::Malformed);
            }
            if self.done && offset < chunk.len() {
                return Err(LinuxConnectivityEvidenceError::Malformed);
            }
        }
        Ok(self.done)
    }

    pub(crate) fn finish(self) -> Result<Vec<SanitizedRouteRow>, LinuxConnectivityEvidenceError> {
        if !self.done {
            return Err(LinuxConnectivityEvidenceError::Incomplete);
        }
        Ok(self.rows)
    }
}

fn parse_kernel_error(payload: &[u8]) -> Result<(), LinuxConnectivityEvidenceError> {
    if payload.len() < size_of::<i32>() {
        return Err(LinuxConnectivityEvidenceError::Malformed);
    }
    if read_i32(payload, 0)? == 0 {
        Ok(())
    } else {
        Err(LinuxConnectivityEvidenceError::KernelRejected)
    }
}

fn parse_route_message(
    payload: &[u8],
    rows: &mut Vec<SanitizedRouteRow>,
) -> Result<(), LinuxConnectivityEvidenceError> {
    if payload.len() < ROUTE_MESSAGE_LEN {
        return Err(LinuxConnectivityEvidenceError::Malformed);
    }
    let family = match payload[0] {
        AF_INET => SanitizedAddressFamily::Ipv4,
        AF_INET6 => SanitizedAddressFamily::Ipv6,
        _ => return Ok(()),
    };
    if payload[1] != 0 || payload[7] != RTN_UNICAST {
        return Ok(());
    }

    let mut table = payload[4] as u32;
    let mut output_interface = None;
    let mut offset = ROUTE_MESSAGE_LEN;
    while offset < payload.len() {
        if payload.len() - offset < ROUTE_ATTRIBUTE_HEADER_LEN {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
        let attribute_len = read_u16(payload, offset)? as usize;
        if attribute_len < ROUTE_ATTRIBUTE_HEADER_LEN || attribute_len > payload.len() - offset {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
        let attribute_type = read_u16(payload, offset + 2)? & NLA_TYPE_MASK;
        let value = &payload[offset + ROUTE_ATTRIBUTE_HEADER_LEN..offset + attribute_len];
        match attribute_type {
            RTA_OIF if value.len() >= size_of::<u32>() => {
                output_interface = Some(read_u32(value, 0)?);
            }
            RTA_TABLE if value.len() >= size_of::<u32>() => {
                table = read_u32(value, 0)?;
            }
            _ => {}
        }

        let aligned_len =
            align4(attribute_len).ok_or(LinuxConnectivityEvidenceError::BudgetExceeded)?;
        let aligned_end = offset
            .checked_add(aligned_len)
            .ok_or(LinuxConnectivityEvidenceError::BudgetExceeded)?;
        if aligned_end <= payload.len() {
            offset = aligned_end;
        } else if offset + attribute_len == payload.len() {
            offset += attribute_len;
        } else {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
    }

    if table == RT_TABLE_MAIN
        && let Some(output_interface) = output_interface.filter(|index| *index != 0)
    {
        let row = SanitizedRouteRow {
            family,
            output_interface,
        };
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    Ok(())
}

fn cached_snapshot(
    cache: &Mutex<Option<CachedConnectivity>>,
    now: Instant,
) -> Option<AgentHostConnectivitySnapshot> {
    cache
        .lock()
        .ok()?
        .as_ref()
        .filter(|entry| entry.expires_at > now)
        .map(|entry| entry.snapshot.clone())
}

fn store_cached_snapshot(
    cache: &Mutex<Option<CachedConnectivity>>,
    snapshot: AgentHostConnectivitySnapshot,
    now: Instant,
    success_ttl: Duration,
    failure_ttl: Duration,
) {
    let ttl = if snapshot.status == AgentHostConnectivityStatus::Indeterminate {
        failure_ttl
    } else {
        success_ttl
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

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|aligned| aligned & !3)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LinuxConnectivityEvidenceError> {
    let value = bytes
        .get(offset..offset + size_of::<u16>())
        .ok_or(LinuxConnectivityEvidenceError::Malformed)?;
    Ok(u16::from_ne_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LinuxConnectivityEvidenceError> {
    let value = bytes
        .get(offset..offset + size_of::<u32>())
        .ok_or(LinuxConnectivityEvidenceError::Malformed)?;
    Ok(u32::from_ne_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, LinuxConnectivityEvidenceError> {
    let value = bytes
        .get(offset..offset + size_of::<i32>())
        .ok_or(LinuxConnectivityEvidenceError::Malformed)?;
    Ok(i32::from_ne_bytes([value[0], value[1], value[2], value[3]]))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + size_of::<u16>()].copy_from_slice(&value.to_ne_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + size_of::<u32>()].copy_from_slice(&value.to_ne_bytes());
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use super::{
        LinuxConnectivityCollector, LinuxConnectivityEvidenceError, LinuxHostConnectivityCore,
        MAX_ROUTE_DUMP_BYTES, ROUTE_DUMP_SEQUENCE, RouteDumpParser, SanitizedAddressFamily,
        SanitizedInterfaceRow, SanitizedRouteRow, merge_linux_evidence, route_dump_request,
    };
    use crate::features::agent::{
        host_connectivity::{HostConnectivityEvidence, diagnose_host_connectivity},
        model::{AgentHostConnectivitySnapshot, AgentHostConnectivityStatus},
    };

    struct TestCollector {
        calls: Arc<AtomicUsize>,
        delay: Duration,
        results: Mutex<VecDeque<AgentHostConnectivitySnapshot>>,
    }

    #[async_trait::async_trait]
    impl LinuxConnectivityCollector for TestCollector {
        async fn collect(&self) -> AgentHostConnectivitySnapshot {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let mut results = self.results.lock().expect("collector results");
            results
                .pop_front()
                .or_else(|| results.back().cloned())
                .unwrap_or_else(unavailable)
        }
    }

    fn unavailable() -> AgentHostConnectivitySnapshot {
        diagnose_host_connectivity(HostConnectivityEvidence::default())
    }

    fn connected() -> AgentHostConnectivitySnapshot {
        diagnose_host_connectivity(HostConnectivityEvidence {
            other_interface_connected: true,
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

    fn collector(
        delay: Duration,
        results: Vec<AgentHostConnectivitySnapshot>,
    ) -> (Arc<TestCollector>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(TestCollector {
                calls: calls.clone(),
                delay,
                results: Mutex::new(results.into()),
            }),
            calls,
        )
    }

    #[test]
    fn sanitized_rows_bind_default_routes_only_to_active_interfaces() {
        let evidence = merge_linux_evidence(
            &[
                SanitizedInterfaceRow {
                    interface_index: 7,
                    up: true,
                    family: Some(SanitizedAddressFamily::Ipv4),
                    usable_address: true,
                },
                SanitizedInterfaceRow {
                    interface_index: 8,
                    up: false,
                    family: Some(SanitizedAddressFamily::Ipv6),
                    usable_address: true,
                },
            ],
            &[
                SanitizedRouteRow {
                    family: SanitizedAddressFamily::Ipv4,
                    output_interface: 7,
                },
                SanitizedRouteRow {
                    family: SanitizedAddressFamily::Ipv6,
                    output_interface: 8,
                },
            ],
        )
        .expect("sanitized evidence");

        assert!(evidence.other_interface_connected);
        assert!(evidence.ipv4_usable_address);
        assert!(evidence.ipv4_default_route);
        assert!(!evidence.ipv6_usable_address);
        assert!(!evidence.ipv6_default_route);
        assert_eq!(evidence.dns_configured, None);
        assert_eq!(evidence.dns_resolves, None);
        assert_eq!(evidence.ipv4_internet_reachable, None);
        assert_eq!(evidence.ipv6_internet_reachable, None);
        assert_eq!(evidence.captive_portal_suspected, None);
        assert!(evidence.probe_complete);
        assert_eq!(
            diagnose_host_connectivity(evidence).status,
            AgentHostConnectivityStatus::Indeterminate
        );
    }

    #[test]
    fn route_parser_accepts_only_main_table_default_routes_and_active_output_indices() {
        let mut parser = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        let mut chunk = route_message(ROUTE_DUMP_SEQUENCE, 2, 0, 254, 7);
        chunk.extend(route_message(ROUTE_DUMP_SEQUENCE, 10, 0, 254, 8));
        chunk.extend(route_message(ROUTE_DUMP_SEQUENCE, 2, 24, 254, 9));
        chunk.extend(route_message(ROUTE_DUMP_SEQUENCE, 2, 0, 100, 10));
        chunk.extend(done_message(ROUTE_DUMP_SEQUENCE));

        assert!(parser.push_chunk(&chunk).expect("parse route dump"));
        assert_eq!(
            parser.finish().expect("complete route dump"),
            vec![
                SanitizedRouteRow {
                    family: SanitizedAddressFamily::Ipv4,
                    output_interface: 7,
                },
                SanitizedRouteRow {
                    family: SanitizedAddressFamily::Ipv6,
                    output_interface: 8,
                },
            ]
        );
    }

    #[test]
    fn route_parser_rejects_malformed_kernel_errors_and_budget_overflow() {
        let mut malformed = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        assert_eq!(
            malformed.push_chunk(&[15, 0, 0, 0]),
            Err(LinuxConnectivityEvidenceError::Malformed)
        );

        let mut rejected = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        assert_eq!(
            rejected.push_chunk(&error_message(ROUTE_DUMP_SEQUENCE, -1)),
            Err(LinuxConnectivityEvidenceError::KernelRejected)
        );

        let mut interrupted = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        assert_eq!(
            interrupted.push_chunk(&interrupted_message(ROUTE_DUMP_SEQUENCE)),
            Err(LinuxConnectivityEvidenceError::Incomplete)
        );

        let mut overrun = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        assert_eq!(
            overrun.push_chunk(&overrun_message(ROUTE_DUMP_SEQUENCE)),
            Err(LinuxConnectivityEvidenceError::Incomplete)
        );

        let mut oversized = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
        assert_eq!(
            oversized.push_chunk(&vec![0; MAX_ROUTE_DUMP_BYTES + 1]),
            Err(LinuxConnectivityEvidenceError::BudgetExceeded)
        );
    }

    #[test]
    fn route_request_is_fixed_bounded_and_read_only() {
        let request = route_dump_request(ROUTE_DUMP_SEQUENCE);
        assert_eq!(request.len(), 28);
        assert_eq!(u16::from_ne_bytes([request[4], request[5]]), 26);
        assert_eq!(u16::from_ne_bytes([request[6], request[7]]), 0x0301);
        assert_eq!(
            u32::from_ne_bytes(request[8..12].try_into().unwrap()),
            ROUTE_DUMP_SEQUENCE
        );
        assert_eq!(request[16], 0);
        assert!(request[17..].iter().all(|byte| *byte == 0));
    }

    #[tokio::test]
    async fn concurrent_snapshots_share_one_probe_and_cached_result() {
        let (collector, calls) = collector(Duration::from_millis(40), vec![connected()]);
        let core = Arc::new(LinuxHostConnectivityCore::new(
            collector,
            Duration::from_millis(200),
        ));

        let (first, second) = tokio::join!(core.snapshot(), core.snapshot());
        assert_eq!(
            serde_json::to_value(&first).expect("serialize first snapshot"),
            serde_json::to_value(&second).expect("serialize second snapshot")
        );
        assert_eq!(first.status, AgentHostConnectivityStatus::OnlineIpv4Only);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            serde_json::to_value(core.snapshot().await).expect("serialize cached snapshot"),
            serde_json::to_value(first).expect("serialize original snapshot")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_keeps_single_flight_until_background_probe_populates_cache() {
        let (collector, calls) = collector(Duration::from_millis(80), vec![connected()]);
        let core = LinuxHostConnectivityCore::new(collector, Duration::from_millis(20));

        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::Indeterminate
        );
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::Indeterminate
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(90)).await;
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::OnlineIpv4Only
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn deterministic_cache_expiry_uses_a_shorter_ttl_for_indeterminate_results() {
        let now = Arc::new(Mutex::new(Instant::now()));
        let clock_now = now.clone();
        let clock: super::Clock = Arc::new(move || *clock_now.lock().expect("clock"));
        let (collector, calls) = collector(
            Duration::ZERO,
            vec![connected(), unavailable(), connected()],
        );
        let core = LinuxHostConnectivityCore::with_policy(
            collector,
            Duration::from_millis(100),
            Duration::from_secs(10),
            Duration::from_secs(2),
            clock,
        );

        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::OnlineIpv4Only
        );
        *now.lock().expect("clock") += Duration::from_secs(9);
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::OnlineIpv4Only
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        *now.lock().expect("clock") += Duration::from_secs(2);
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::Indeterminate
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        *now.lock().expect("clock") += Duration::from_secs(1);
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::Indeterminate
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        *now.lock().expect("clock") += Duration::from_secs(2);
        assert_eq!(
            core.snapshot().await.status,
            AgentHostConnectivityStatus::OnlineIpv4Only
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    fn route_message(
        sequence: u32,
        family: u8,
        destination_len: u8,
        table: u8,
        oif: u32,
    ) -> Vec<u8> {
        let attribute_len = 8_usize;
        let message_len = 16 + 12 + attribute_len;
        let mut message = vec![0_u8; message_len];
        write_u32(&mut message, 0, message_len as u32);
        write_u16(&mut message, 4, 24);
        write_u32(&mut message, 8, sequence);
        message[16] = family;
        message[17] = destination_len;
        message[20] = table;
        message[23] = 1;
        write_u16(&mut message, 28, attribute_len as u16);
        write_u16(&mut message, 30, 4);
        write_u32(&mut message, 32, oif);
        message
    }

    fn done_message(sequence: u32) -> Vec<u8> {
        let mut message = vec![0_u8; 16];
        write_u32(&mut message, 0, 16);
        write_u16(&mut message, 4, 3);
        write_u32(&mut message, 8, sequence);
        message
    }

    fn error_message(sequence: u32, error: i32) -> Vec<u8> {
        let mut message = vec![0_u8; 20];
        write_u32(&mut message, 0, 20);
        write_u16(&mut message, 4, 2);
        write_u32(&mut message, 8, sequence);
        message[16..20].copy_from_slice(&error.to_ne_bytes());
        message
    }

    fn interrupted_message(sequence: u32) -> Vec<u8> {
        let mut message = done_message(sequence);
        write_u16(&mut message, 6, 0x0010);
        message
    }

    fn overrun_message(sequence: u32) -> Vec<u8> {
        let mut message = vec![0_u8; 16];
        write_u32(&mut message, 0, 16);
        write_u16(&mut message, 4, 4);
        write_u32(&mut message, 8, sequence);
        message
    }

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_ne_bytes());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
}

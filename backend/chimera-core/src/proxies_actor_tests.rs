use super::*;
use axum::{
    Json, Router,
    extract::{Path, State as HttpState},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, put},
};
use clash_api::ProxyName;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

const GROUP: &str = "group/日本 ?#";
const NODE: &str = "node/日本";
const PROVIDER: &str = "provider/日本 ?#";

struct TestApiSource {
    api: tokio::sync::RwLock<ApiClient>,
    server: std::net::SocketAddr,
}

#[async_trait::async_trait]
impl ProxyApiClientSource for TestApiSource {
    async fn api_client(&self) -> Result<ApiClient> {
        Ok(self.api.read().await.clone())
    }
}

impl TestApiSource {
    async fn rotate(&self) {
        self.api.read().await.revoke();
        let next = ApiClient::new_unbound(self.server.to_string(), None).unwrap();
        *self.api.write().await = next;
    }
}
#[derive(Default)]
struct Fixture {
    reads: AtomicUsize,
    fail_reads: AtomicBool,
    fail_mutation: AtomicBool,
    accepted: AtomicBool,
    fail_close: AtomicBool,
    hold_connections: AtomicBool,
    closed: Mutex<Vec<uuid::Uuid>>,
    hold_read: AtomicBool,
    hold_select: AtomicBool,
    hold_provider: AtomicBool,
    entered: Notify,
    release: Notify,
    calls: Mutex<Vec<&'static str>>,
    selected: Mutex<String>,
    subscription_expire: AtomicUsize,
    provider_nodes: Mutex<Option<Vec<serde_json::Value>>>,
    fixed: Mutex<String>,
    group_list: Mutex<Option<serde_json::Value>>,
}
async fn proxies(HttpState(f): HttpState<Arc<Fixture>>) -> Response {
    f.reads.fetch_add(1, Ordering::SeqCst);
    f.calls.lock().unwrap().push("read");
    if f.hold_read.load(Ordering::SeqCst) {
        f.entered.notify_one();
        f.release.notified().await;
    }
    if f.fail_reads.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut proxies = serde_json::json!({});
    for (name, kind) in [
        ("DIRECT", "Direct"),
        ("REJECT", "Reject"),
        ("GLOBAL", "Selector"),
        (GROUP, "URLTest"),
    ] {
        proxies[name] = serde_json::json!({"name":name,"type":kind,"udp":true,"history":[]});
    }
    proxies["GLOBAL"]["all"] = serde_json::json!([GROUP]);
    proxies[GROUP]["all"] = serde_json::json!([NODE, "DIRECT"]);
    proxies[GROUP]["now"] = serde_json::json!(f.selected.lock().unwrap().clone());
    proxies[GROUP]["fixed"] = serde_json::json!(f.fixed.lock().unwrap().clone());
    Json(serde_json::json!({"proxies":proxies})).into_response()
}
async fn groups(HttpState(f): HttpState<Arc<Fixture>>) -> Response {
    match f.group_list.lock().unwrap().clone() {
        Some(list) => Json(list).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn providers(HttpState(f): HttpState<Arc<Fixture>>) -> Json<serde_json::Value> {
    let nodes = f.provider_nodes.lock().unwrap().clone().unwrap_or_else(|| {
        vec![serde_json::json!({"name":NODE,"type":"Vless","udp":true,"history":[]})]
    });
    Json(
        serde_json::json!({"providers":{PROVIDER:{"name":PROVIDER,"type":"Proxy","vehicleType":"HTTP", "proxies":nodes, "subscriptionInfo":{"Expire":f.subscription_expire.load(Ordering::SeqCst),"Upload":-1}}}}),
    )
}
async fn select(
    HttpState(f): HttpState<Arc<Fixture>>,
    Path(group): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    assert_eq!(group, GROUP);
    assert_eq!(body["name"], NODE);
    f.calls.lock().unwrap().push("select");
    if f.hold_select.load(Ordering::SeqCst) {
        f.entered.notify_one();
        f.release.notified().await;
    }
    if f.fail_mutation.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    *f.selected.lock().unwrap() = NODE.into();
    *f.fixed.lock().unwrap() = NODE.into();
    StatusCode::NO_CONTENT
}
async fn clear(HttpState(f): HttpState<Arc<Fixture>>, Path(group): Path<String>) -> StatusCode {
    assert_eq!(group, GROUP);
    f.calls.lock().unwrap().push("clear");
    if f.fail_mutation.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    f.fixed.lock().unwrap().clear();
    StatusCode::NO_CONTENT
}
async fn update(HttpState(f): HttpState<Arc<Fixture>>, Path(name): Path<String>) -> StatusCode {
    assert_eq!(name, PROVIDER);
    f.calls.lock().unwrap().push("update");
    if f.hold_provider.load(Ordering::SeqCst) {
        f.entered.notify_one();
        f.release.notified().await;
    }
    if f.fail_mutation.load(Ordering::SeqCst) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NO_CONTENT
    }
}
async fn healthcheck(
    HttpState(f): HttpState<Arc<Fixture>>,
    Path(name): Path<String>,
) -> StatusCode {
    assert_eq!(name, PROVIDER);
    f.calls.lock().unwrap().push("healthcheck");
    if f.hold_provider.load(Ordering::SeqCst) {
        f.entered.notify_one();
        f.release.notified().await;
    }
    if f.fail_mutation.load(Ordering::SeqCst) {
        StatusCode::SERVICE_UNAVAILABLE
    } else if f.accepted.load(Ordering::SeqCst) {
        StatusCode::ACCEPTED
    } else {
        StatusCode::NO_CONTENT
    }
}
async fn close(HttpState(f): HttpState<Arc<Fixture>>) -> StatusCode {
    f.calls.lock().unwrap().push("close");
    if f.fail_close.load(Ordering::SeqCst) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NO_CONTENT
    }
}
async fn connections(HttpState(f): HttpState<Arc<Fixture>>) -> Json<serde_json::Value> {
    f.calls.lock().unwrap().push("connections");
    if f.hold_connections.load(Ordering::SeqCst) {
        f.entered.notify_one();
        f.release.notified().await;
    }
    let connections: Vec<_> = [vec![NODE, GROUP, "GLOBAL"], vec!["other"], vec![GROUP]]
        .into_iter()
        .enumerate()
        .map(|(index, chains)| {
            serde_json::json!({
                "id": uuid::Uuid::from_u128(index as u128 + 1),
                "upload": 0, "download": 0, "start": "2026-01-01T00:00:00Z",
                "chains": chains, "rule": "Match", "rulePayload": ""
            })
        })
        .collect();
    Json(serde_json::json!({"downloadTotal": 0, "uploadTotal": 0, "connections": connections}))
}
async fn close_one(
    HttpState(f): HttpState<Arc<Fixture>>,
    Path(id): Path<uuid::Uuid>,
) -> StatusCode {
    f.closed.lock().unwrap().push(id);
    close(HttpState(f)).await
}
async fn setup() -> (
    ProxiesClient,
    CoreClient,
    Arc<TestApiSource>,
    Arc<Fixture>,
    tokio::task::JoinHandle<()>,
) {
    let fixture = Arc::new(Fixture {
        subscription_expire: AtomicUsize::new(42),
        ..Fixture::default()
    });
    let router = Router::new()
        .route("/group", get(groups))
        .route("/proxies", get(proxies))
        .route("/providers/proxies", get(providers))
        .route("/proxies/{group}", put(select).delete(clear))
        .route("/providers/proxies/{name}", put(update))
        .route("/providers/proxies/{name}/healthcheck", get(healthcheck))
        .route("/connections", get(connections).delete(close))
        .route("/connections/{id}", delete(close_one))
        .with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let source = Arc::new(TestApiSource {
        api: tokio::sync::RwLock::new(
            ApiClient::new_unbound(server_address.to_string(), None).unwrap(),
        ),
        server: server_address,
    });
    let core = CoreClient::from_source(source.clone());
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = ProxiesClient::spawn(core.clone(), shutdown, &tasks)
        .await
        .unwrap();
    (client, core, source, fixture, server)
}
#[tokio::test]
async fn clearing_a_pin_follows_the_break_strategy_then_refreshes() {
    let (client, _core, _, fixture, server) = setup().await;
    client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
        .await
        .unwrap();
    assert_eq!(
        client.snapshot().groups[0]
            .fixed
            .as_ref()
            .map(ProxyName::as_str),
        Some(NODE)
    );
    fixture.calls.lock().unwrap().clear();
    client
        .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "close", "read"]);
    assert_eq!(client.snapshot().groups[0].fixed, None);
    fixture.calls.lock().unwrap().clear();
    client
        .clear_fixed(GROUP.into(), ProxyChangeBreakMode::Off)
        .await
        .unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "read"]);
    server.abort();
}
#[tokio::test]
async fn clearing_a_pin_closes_only_the_group_chains() {
    let (client, _core, _, fixture, server) = setup().await;
    let outcome = client
        .clear_fixed(GROUP.into(), ProxyChangeBreakMode::ProxyGroup)
        .await
        .unwrap();
    assert!(outcome.degradations().is_empty());
    assert_eq!(
        *fixture.closed.lock().unwrap(),
        [uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3)]
    );
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["clear", "connections", "close", "close", "read"]
    );
    server.abort();
}
#[tokio::test]
async fn a_rejected_unpin_rereads_the_core_and_keeps_the_snapshot() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.fail_mutation.store(true, Ordering::SeqCst);
    fixture.calls.lock().unwrap().clear();
    let error = client
        .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("clear_proxy_selection"),
        "{error:#}"
    );
    assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "read"]);
    assert!(!client.snapshot().groups.is_empty());
    server.abort();
}
#[tokio::test]
async fn an_unpin_whose_interruption_fails_is_degraded_not_failed() {
    let (client, _core, _, fixture, server) = setup().await;
    fixture.fail_close.store(true, Ordering::SeqCst);
    let outcome = client
        .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap();
    assert_eq!(outcome.degradations().len(), 1);
    assert_eq!(outcome.degradations()[0].code, "proxy_interruption_failed");
    assert!(
        outcome.degradations()[0]
            .message
            .starts_with("pinned selection cleared, but"),
        "{}",
        outcome.degradations()[0].message
    );
    assert!(!client.snapshot().nodes.is_empty());
    server.abort();
}
#[tokio::test]
async fn cache_ttl_and_provider_metadata_are_shared() {
    let (client, _core, _, fixture, server) = setup().await;
    let proxies = client.get(false).await.unwrap();
    assert_eq!(proxies.groups[0].all[0].as_str(), NODE);
    assert_eq!(
        proxies.nodes[&ProxyName::from(NODE)].provider.as_deref(),
        Some(PROVIDER)
    );
    let providers = client.providers().await.unwrap();
    assert_eq!(providers[&ProviderName::from(PROVIDER)].proxy_count, 1);
    let info = providers[&clash_api::ProviderName::from(PROVIDER)]
        .subscription_info
        .as_ref()
        .unwrap();
    assert_eq!((info.expire, info.upload), (42, -1));
    client.get(false).await.unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::time::resume();
    client.get(false).await.unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 2);
    client.get(false).await.unwrap();
    assert_eq!(
        fixture.reads.load(Ordering::SeqCst),
        2,
        "unchanged refresh must renew freshness"
    );
    server.abort();
}
#[tokio::test]
async fn concurrent_reads_share_one_actor_snapshot_refresh() {
    let (client, _core, _, fixture, server) = setup().await;
    let reads = (0..8).map(|_| {
        let client = client.clone();
        tokio::spawn(async move { client.get(false).await.unwrap() })
    });
    for read in reads {
        assert_eq!(read.await.unwrap().groups[0].all[0].as_str(), NODE);
    }
    assert_eq!(
        fixture.reads.load(Ordering::SeqCst),
        1,
        "concurrent callers must share the actor's initial snapshot"
    );
    server.abort();
}

#[tokio::test]
async fn profile_invalidation_clears_the_actor_snapshot_before_the_next_read() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    let mut changes = client.subscribe();
    changes.borrow_and_update();

    client.invalidate();

    tokio::time::timeout(Duration::from_secs(3), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(client.snapshot().nodes.is_empty());
    client.get(false).await.unwrap();
    assert_eq!(
        fixture.reads.load(Ordering::SeqCst),
        2,
        "the first read after profile invalidation must fetch a new snapshot"
    );
    assert!(!client.snapshot().nodes.is_empty());
    server.abort();
}

#[tokio::test]
async fn a_burst_of_refresh_requests_rereads_once() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    for _ in 0..5 {
        client.request_refresh();
    }
    // Answered after the queued refresh, from the snapshot it just read.
    client.get(false).await.unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 2);
    client.request_refresh();
    client.get(false).await.unwrap();
    assert_eq!(
        fixture.reads.load(Ordering::SeqCst),
        3,
        "a request after the reread queues another"
    );
    server.abort();
}
#[tokio::test]
async fn provider_summaries_keep_node_counts_and_notify_only_for_application_changes() {
    let (client, _core, _, fixture, server) = setup().await;
    let node = |name, delay| {
        serde_json::json!({
            "name": name, "type": "Vless", "udp": true,
            "history": [{"time": "2026-10-18T00:00:00Z", "delay": delay}],
        })
    };
    *fixture.provider_nodes.lock().unwrap() = Some(vec![node(NODE, 10), node("unused", 20)]);
    let mut changes = client.subscribe();

    let proxies = client.get(true).await.unwrap();
    assert_eq!(proxies.nodes[&ProxyName::from(NODE)].history[0].delay, 10);
    assert!(!proxies.nodes.contains_key(&ProxyName::from("unused")));
    let providers = client.providers().await.unwrap();
    assert_eq!(providers[&ProviderName::from(PROVIDER)].proxy_count, 2);
    changes.borrow_and_update();

    // An unused node's history belongs only to the transient core response.
    *fixture.provider_nodes.lock().unwrap() = Some(vec![node(NODE, 10), node("unused", 21)]);
    client.get(true).await.unwrap();
    assert!(!changes.has_changed().unwrap());

    *fixture.provider_nodes.lock().unwrap() = Some(vec![node(NODE, 11), node("unused", 21)]);
    let proxies = client.get(true).await.unwrap();
    assert_eq!(proxies.nodes[&ProxyName::from(NODE)].history[0].delay, 11);
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();

    fixture
        .provider_nodes
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .push(node("another unused", 30));
    client.get(true).await.unwrap();
    let providers = client.providers().await.unwrap();
    assert_eq!(providers[&ProviderName::from(PROVIDER)].proxy_count, 3);
    assert!(changes.has_changed().unwrap());

    client.stop_for_test().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn refresh_notifies_only_when_proxies_or_providers_change() {
    let (client, _core, _, fixture, server) = setup().await;
    let mut changes = client.subscribe();

    client.get(true).await.unwrap();
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();

    client.get(true).await.unwrap();
    assert!(!changes.has_changed().unwrap());

    *fixture.selected.lock().unwrap() = NODE.into();
    client.get(true).await.unwrap();
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();

    client.get(true).await.unwrap();
    assert!(!changes.has_changed().unwrap());

    let proxies_before = serde_json::to_value(client.snapshot()).unwrap();
    fixture.subscription_expire.store(43, Ordering::SeqCst);
    client.get(true).await.unwrap();
    assert_eq!(
        serde_json::to_value(client.snapshot()).unwrap(),
        proxies_before
    );
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();

    client.get(true).await.unwrap();
    assert!(!changes.has_changed().unwrap());

    client.stop_for_test().await.unwrap();
    server.abort();
}
/// The arrangement is not part of the snapshot, so the subscribers (the
/// tray) are woken by the owner on request.
#[tokio::test]
async fn a_rearrangement_wakes_subscribers_although_the_snapshot_is_unchanged() {
    let (client, _core, _, _, server) = setup().await;
    let mut changes = client.subscribe();
    client.get(true).await.unwrap();
    changes.borrow_and_update();

    client.notify_rearranged();
    tokio::time::timeout(Duration::from_secs(5), changes.changed())
        .await
        .expect("the subscribers are woken")
        .unwrap();

    client.stop_for_test().await.unwrap();
    server.abort();
}
#[tokio::test]
async fn select_closes_then_refreshes_without_replaying() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.calls.lock().unwrap().clear();
    client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["select", "close", "read"]);
    assert_eq!(
        client.snapshot().groups[0]
            .now
            .as_ref()
            .map(ProxyName::as_str),
        Some(NODE)
    );
    fixture.calls.lock().unwrap().clear();
    client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
        .await
        .unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["select", "read"]);
    fixture.calls.lock().unwrap().clear();
    client.update_provider(PROVIDER.into()).await.unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["update", "read"]);
    server.abort();
}
#[tokio::test]
async fn successful_mutation_with_failed_refresh_is_explicit_and_clears_cache() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.fail_reads.store(true, Ordering::SeqCst);
    let outcome = client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap();
    assert_eq!(outcome.degradations()[0].code, "proxy_cache_refresh_failed");
    assert!(client.snapshot().nodes.is_empty());
    assert_eq!(
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|&&call| call == "select")
            .count(),
        1
    );
    fixture.calls.lock().unwrap().clear();
    let error = client.update_provider(PROVIDER.into()).await.unwrap_err();
    assert!(error.to_string().contains("provider update succeeded"));
    assert!(client.snapshot().nodes.is_empty());
    assert_eq!(*fixture.calls.lock().unwrap(), ["update", "read"]);
    fixture.calls.lock().unwrap().clear();
    fixture.fail_mutation.store(true, Ordering::SeqCst);
    let error = client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("select_proxy"), "{error:#}");
    assert_eq!(*fixture.calls.lock().unwrap(), ["select", "read"]);
    assert!(
        client.snapshot().nodes.is_empty(),
        "the re-read failed, so the cache is cleared"
    );
    server.abort();
}
#[tokio::test]
async fn a_selection_keeps_the_published_snapshot_until_the_core_answers() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    let before = serde_json::to_value(client.snapshot()).unwrap();
    let mut changes = client.subscribe();
    changes.borrow_and_update();
    fixture.hold_select.store(true, Ordering::SeqCst);
    let selecting = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
                .await
        })
    };
    fixture.entered.notified().await;
    assert!(!client.snapshot().groups.is_empty());
    assert_eq!(serde_json::to_value(client.snapshot()).unwrap(), before);
    assert!(!changes.has_changed().unwrap());
    fixture.hold_select.store(false, Ordering::SeqCst);
    fixture.release.notify_one();
    selecting.await.unwrap().unwrap();
    assert_eq!(
        client.snapshot().groups[0]
            .now
            .as_ref()
            .map(ProxyName::as_str),
        Some(NODE)
    );
    assert!(changes.has_changed().unwrap());
    server.abort();
}
#[tokio::test]
async fn a_rejected_mutation_rereads_the_core_instead_of_emptying_the_snapshot() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.fail_mutation.store(true, Ordering::SeqCst);
    fixture.calls.lock().unwrap().clear();
    let error = client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("select_proxy"), "{error:#}");
    assert_eq!(*fixture.calls.lock().unwrap(), ["select", "read"]);
    assert!(!client.snapshot().groups.is_empty());
    fixture.calls.lock().unwrap().clear();
    let error = client.update_provider(PROVIDER.into()).await.unwrap_err();
    assert!(
        error.to_string().contains("update_proxy_provider"),
        "{error:#}"
    );
    assert_eq!(*fixture.calls.lock().unwrap(), ["update", "read"]);
    assert!(!client.snapshot().groups.is_empty());
    server.abort();
}
#[tokio::test]
async fn a_provider_health_check_rereads_the_core() {
    let (client, _core, _, fixture, server) = setup().await;
    client.healthcheck_provider(PROVIDER.into()).await.unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["healthcheck", "read"]);
    fixture.calls.lock().unwrap().clear();
    fixture.accepted.store(true, Ordering::SeqCst);
    client.healthcheck_provider(PROVIDER.into()).await.unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), ["healthcheck", "read"]);
    server.abort();
}
#[tokio::test]
async fn a_rejected_health_check_rereads_and_reports() {
    let (client, _core, _, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.fail_mutation.store(true, Ordering::SeqCst);
    fixture.calls.lock().unwrap().clear();
    let error = client
        .healthcheck_provider(PROVIDER.into())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("healthcheck_proxy_provider"),
        "{error:#}"
    );
    assert_eq!(*fixture.calls.lock().unwrap(), ["healthcheck", "read"]);
    assert!(!client.snapshot().groups.is_empty());
    server.abort();
}
#[tokio::test]
async fn group_policy_closes_only_matching_chains() {
    let (client, _core, _, fixture, server) = setup().await;
    let outcome = client
        .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::ProxyGroup)
        .await
        .unwrap();
    assert!(outcome.degradations().is_empty());
    assert_eq!(
        *fixture.closed.lock().unwrap(),
        [uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3)]
    );
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["select", "connections", "close", "close", "read"]
    );
    server.abort();
}
#[tokio::test]
async fn interruption_failure_preserves_selection_and_refreshes_cache() {
    for strategy in [ProxyChangeBreakMode::All, ProxyChangeBreakMode::ProxyGroup] {
        let (client, _core, _, fixture, server) = setup().await;
        fixture.fail_close.store(true, Ordering::SeqCst);
        let outcome = client
            .select(GROUP.into(), NODE.into(), strategy)
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(outcome.degradations()[0].code, "proxy_interruption_failed");
        assert!(!outcome.degradations()[0].retryable);
        assert_eq!(*fixture.selected.lock().unwrap(), NODE);
        assert!(!client.snapshot().nodes.is_empty());
        server.abort();
    }
}
#[tokio::test]
async fn replacement_during_chain_read_never_receives_closure_or_cache() {
    let (client, _core, source, fixture, server) = setup().await;
    fixture.hold_connections.store(true, Ordering::SeqCst);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::ProxyGroup)
                .await
        })
    };
    fixture.entered.notified().await;
    source.rotate().await;
    let outcome = waiting.await.unwrap().unwrap();
    assert_eq!(outcome.degradations().len(), 2);
    assert!(fixture.closed.lock().unwrap().is_empty());
    assert_eq!(*fixture.calls.lock().unwrap(), ["select", "connections"]);
    assert!(client.snapshot().nodes.is_empty());
    fixture.release.notify_one();
    server.abort();
}
#[tokio::test]
async fn shutdown_during_connection_interruption_does_not_refresh_or_report_success() {
    let (client, _core, _, fixture, server) = setup().await;
    fixture.hold_connections.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::ProxyGroup)
                .await
        })
    };
    fixture.entered.notified().await;
    client.0.shutdown.cancel();
    fixture.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    // The controller may have completed connection closes before cancellation;
    // never claim those side effects were rolled back or retry them.
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["select", "connections", "close", "close"]
    );
    assert_eq!(fixture.closed.lock().unwrap().len(), 2);
    assert!(client.snapshot().nodes.is_empty());
    server.abort();
}

#[tokio::test]
async fn idle_cache_is_cleared_when_its_capability_is_revoked() {
    let (client, _core, source, _, server) = setup().await;
    client.get(false).await.unwrap();
    let mut changes = client.subscribe();
    changes.borrow_and_update();
    source.rotate().await;
    assert!(client.snapshot().nodes.is_empty());
    tokio::time::timeout(Duration::from_secs(3), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(client.snapshot().nodes.is_empty());
    server.abort();
}

#[tokio::test]
async fn replacement_during_read_never_publishes_the_old_response() {
    let (client, _core, source, fixture, server) = setup().await;
    client.get(false).await.unwrap();
    fixture.hold_read.store(true, Ordering::SeqCst);
    let waiting = {
        let client = client.clone();
        tokio::spawn(async move { client.get(true).await })
    };
    fixture.entered.notified().await;
    source.rotate().await;
    assert!(waiting.await.unwrap().is_err());
    assert!(client.snapshot().nodes.is_empty());
    fixture.hold_read.store(false, Ordering::SeqCst);
    fixture.release.notify_one();
    client.get(false).await.unwrap();
    assert!(!client.snapshot().nodes.is_empty());
    server.abort();
}
#[tokio::test]
async fn shutdown_during_controller_read_cannot_publish_a_snapshot() {
    let (client, _core, _, fixture, server) = setup().await;
    fixture.hold_read.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { client.get(true).await })
    };
    fixture.entered.notified().await;
    client.0.shutdown.cancel();
    fixture.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert!(client.snapshot().nodes.is_empty());
    server.abort();
}

#[tokio::test]
async fn shutdown_during_route_write_skips_connection_effects_and_cache_publication() {
    let (client, _core, _, fixture, server) = setup().await;
    fixture.hold_select.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
                .await
        })
    };
    fixture.entered.notified().await;
    client.0.shutdown.cancel();
    fixture.hold_select.store(false, Ordering::SeqCst);
    fixture.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(*fixture.calls.lock().unwrap(), ["select"]);
    assert!(fixture.closed.lock().unwrap().is_empty());
    assert!(client.snapshot().nodes.is_empty());
    server.abort();
}

#[tokio::test]
async fn shutdown_during_provider_mutation_skips_reread_and_cache_publication() {
    for healthcheck in [false, true] {
        let (client, _core, _, fixture, server) = setup().await;
        fixture.hold_provider.store(true, Ordering::SeqCst);
        let pending = {
            let client = client.clone();
            tokio::spawn(async move {
                if healthcheck {
                    client.healthcheck_provider(PROVIDER.into()).await
                } else {
                    client.update_provider(PROVIDER.into()).await
                }
            })
        };
        fixture.entered.notified().await;
        client.0.shutdown.cancel();
        fixture.hold_provider.store(false, Ordering::SeqCst);
        fixture.release.notify_one();
        assert!(pending.await.unwrap().is_err());
        assert_eq!(
            *fixture.calls.lock().unwrap(),
            [if healthcheck { "healthcheck" } else { "update" }]
        );
        assert!(client.snapshot().nodes.is_empty());
        server.abort();
    }
}

#[tokio::test]
async fn queued_cancelled_mutation_is_not_executed_and_shutdown_closes_subscriptions() {
    let (client, _core, _, fixture, server) = setup().await;
    fixture.hold_select.store(true, Ordering::SeqCst);
    let first = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
                .await
        })
    };
    fixture.entered.notified().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    client
        .0
        .actor
        .cast(Message::Select {
            group: GROUP.into(),
            name: NODE.into(),
            strategy: ProxyChangeBreakMode::All,
            reply: tx.into(),
        })
        .unwrap();
    drop(rx);
    fixture.release.notify_one();
    first.await.unwrap().unwrap();
    client.get(false).await.unwrap(); // Acknowledges the queued message was processed.
    assert_eq!(
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|&&call| call == "select")
            .count(),
        1
    );
    let mut changes = client.subscribe();
    changes.borrow_and_update();
    drop(client);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), changes.changed())
            .await
            .unwrap()
            .is_err()
    );
    server.abort();
}
#[tokio::test]
async fn listed_groups_reach_the_published_snapshot() {
    let (client, _core, _, fixture, server) = setup().await;
    *fixture.group_list.lock().unwrap() = Some(serde_json::json!({"proxies":[
        {"name":"listed", "type":"LoadBalance", "udp":true, "history":[], "all":[NODE]}
    ]}));
    let proxies = client.get(true).await.unwrap();
    assert_eq!(proxies.groups.len(), 1);
    assert_eq!(proxies.groups[0].name.as_str(), "listed");
    assert_eq!(
        proxies.nodes[&ProxyName::from(NODE)].provider.as_deref(),
        Some(PROVIDER)
    );
    client.stop_for_test().await.unwrap();
    server.abort();
}

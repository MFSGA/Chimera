//! Ref's isolated Actor behavior tests with injected platform and switch ports.

use std::sync::{Arc, Mutex};

use chimera_config::application::{
    InterfaceKind, MechanismAction, MechanismActions, NetworkAutomation, NetworkConditions,
    NetworkMatch, NetworkObservation, NetworkRule, NetworkState, WifiName, WifiUnavailable,
};
use chimera_contracts::network_automation::{
    MechanismOutcome, MechanismOutcomes, NetworkAutomationStatus,
};
use chimera_contracts::network_environment::{NetworkEnvironmentPort, WifiNameRead};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{NetworkAutomationArgs, NetworkAutomationClient, ports::CaptureSwitches};

const PROXY_OFF: MechanismActions = MechanismActions {
    system_proxy: MechanismAction::Disable,
    tun: MechanismAction::Keep,
    transparent_proxy: MechanismAction::Keep,
};
const PROXY_ON: MechanismActions = MechanismActions {
    system_proxy: MechanismAction::Enable,
    tun: MechanismAction::Keep,
    transparent_proxy: MechanismAction::Keep,
};

fn wifi(ssid: &str) -> NetworkState {
    NetworkState::Online {
        network: NetworkObservation {
            interface_kind: InterfaceKind::Wifi,
            wifi: WifiName::Known { ssid: ssid.into() },
            gateway_mac: None,
        },
    }
}

fn home_rule(actions: MechanismActions) -> NetworkRule {
    NetworkRule {
        id: "home".into(),
        name: "Home".into(),
        enabled: true,
        conditions: NetworkConditions {
            wifi_ssid: Some("Home".into()),
            ..Default::default()
        },
        actions,
    }
}

/// Home switches the proxy off; every other network switches it on.
fn policy() -> NetworkAutomation {
    NetworkAutomation {
        enabled: true,
        default_actions: PROXY_ON,
        rules: vec![home_rule(PROXY_OFF)],
    }
}

fn home() -> NetworkMatch {
    NetworkMatch::Rule { id: "home".into() }
}

/// The network the test says the machine is on. Like the real observer, it
/// leaves a Wi-Fi name unread when the caller skips it, and it records how
/// each reading was requested.
struct FakeEnvironment {
    network: Mutex<NetworkState>,
    requests: Mutex<Vec<WifiNameRead>>,
}

impl FakeEnvironment {
    fn new(network: NetworkState) -> Self {
        Self {
            network: Mutex::new(network),
            requests: Mutex::default(),
        }
    }

    fn requests(&self) -> Vec<WifiNameRead> {
        self.requests.lock().unwrap().clone()
    }
}

impl NetworkEnvironmentPort for FakeEnvironment {
    fn current(&self, wifi_name: WifiNameRead) -> NetworkState {
        self.requests.lock().unwrap().push(wifi_name);
        let mut network = self.network.lock().unwrap().clone();
        if let NetworkState::Online { network } = &mut network
            && wifi_name == WifiNameRead::Skip
            && matches!(network.wifi, WifiName::Known { .. })
        {
            network.wifi = WifiName::NotRead;
        }
        network
    }
}

/// Records every action set it is asked to apply.
#[derive(Default)]
struct RecordingSwitches(Mutex<Vec<MechanismActions>>);

#[async_trait::async_trait]
impl CaptureSwitches for RecordingSwitches {
    async fn apply(&self, actions: MechanismActions) -> MechanismOutcomes {
        self.0.lock().unwrap().push(actions);
        MechanismOutcomes {
            system_proxy: MechanismOutcome::Enabled,
            tun: MechanismOutcome::Unchanged,
            transparent_proxy: MechanismOutcome::Unchanged,
        }
    }
}

struct Fixture {
    client: NetworkAutomationClient,
    environment: Arc<FakeEnvironment>,
    switches: Arc<RecordingSwitches>,
}

impl Fixture {
    async fn spawn(policy: NetworkAutomation, network: NetworkState) -> Self {
        let environment = Arc::new(FakeEnvironment::new(network));
        let switches = Arc::new(RecordingSwitches::default());
        let client = NetworkAutomationClient::spawn(
            NetworkAutomationArgs {
                environment: environment.clone(),
                switches: switches.clone(),
                policy,
                schedule_polls: false,
                shutdown: CancellationToken::new(),
            },
            &TaskTracker::new(),
        )
        .await
        .unwrap();

        Self {
            client,
            environment,
            switches,
        }
    }

    /// Moves the machine and polls until the reading settles.
    async fn arrive(&self, network: NetworkState) {
        *self.environment.network.lock().unwrap() = network;
        self.client.poll().await;
        self.client.poll().await;
    }

    async fn set_policy(&self, policy: NetworkAutomation) -> NetworkAutomationStatus {
        self.client.set_policy(policy);
        self.client.status().await
    }

    fn applied(&self) -> Vec<MechanismActions> {
        self.switches.0.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn a_network_applies_once_after_two_agreeing_polls() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.client.poll().await;
    assert_eq!(fixture.applied(), [], "one reading has not settled");

    fixture.client.poll().await;
    fixture.client.poll().await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    let status = fixture.client.status().await;
    assert!(status.supported);
    assert_eq!(status.network, Some(wifi("Home")));
    assert_eq!(status.applied.unwrap().matched, home());
}

#[tokio::test]
async fn readings_that_keep_changing_apply_nothing() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    for network in [wifi("Cafe"), wifi("Home"), wifi("Cafe"), wifi("Home")] {
        *fixture.environment.network.lock().unwrap() = network;
        fixture.client.poll().await;
    }

    assert_eq!(fixture.applied(), []);
}

#[tokio::test]
async fn each_arrival_on_a_different_match_applies_its_actions() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    fixture.arrive(wifi("Cafe")).await;
    // Another unmatched network is the same match: nothing is written, so a
    // manual switch made at the first one stands.
    fixture.arrive(wifi("Airport")).await;
    fixture.arrive(wifi("Home")).await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON, PROXY_OFF]);
}

#[tokio::test]
async fn offline_holds_the_switches_until_a_different_network() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    fixture.arrive(NetworkState::Offline).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    fixture.arrive(NetworkState::Offline).await;
    fixture.arrive(wifi("Cafe")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON]);
}

#[tokio::test]
async fn a_policy_edit_applies_only_a_changed_answer() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;

    let mut renamed = policy();
    renamed.rules[0].name = "My flat".into();
    renamed.default_actions = MechanismActions::default();
    fixture.set_policy(renamed).await;
    assert_eq!(
        fixture.applied(),
        [PROXY_OFF],
        "the answer for Home is the same"
    );

    let mut tun_too = policy();
    tun_too.rules[0].actions.tun = MechanismAction::Disable;
    fixture.set_policy(tun_too.clone()).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, tun_too.rules[0].actions]);
}

#[tokio::test]
async fn a_disabled_policy_observes_without_writing() {
    let disabled = NetworkAutomation {
        enabled: false,
        ..policy()
    };
    let fixture = Fixture::spawn(disabled.clone(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    let status = fixture.client.apply_now().await;
    assert_eq!(fixture.applied(), []);
    assert_eq!(
        status.network,
        Some(NetworkState::Online {
            network: NetworkObservation {
                interface_kind: InterfaceKind::Wifi,
                wifi: WifiName::NotRead,
                gateway_mac: None,
            }
        }),
        "the editor still sees the network, but nothing asked for its name"
    );
    assert_eq!(status.applied, None);

    fixture.set_policy(policy()).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    // Disabling forgets the application, so enabling again applies again.
    let status = fixture.set_policy(disabled).await;
    assert_eq!(status.applied, None);
    fixture.set_policy(policy()).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_OFF]);
}

#[tokio::test]
async fn apply_now_writes_the_same_answer_again() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;

    let status = fixture.client.apply_now().await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_OFF]);
    assert_eq!(
        status.applied.unwrap().outcomes.system_proxy,
        MechanismOutcome::Enabled
    );
}

#[tokio::test]
async fn apply_now_reads_the_network_without_waiting_to_settle() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;
    *fixture.environment.network.lock().unwrap() = wifi("Cafe");

    let status = fixture.client.apply_now().await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON]);
    assert_eq!(status.applied.unwrap().matched, NetworkMatch::Default);
}

/// Applies to any Wi-Fi network, so it needs no Wi-Fi name.
fn wifi_kind_policy() -> NetworkAutomation {
    NetworkAutomation {
        enabled: true,
        default_actions: MechanismActions::default(),
        rules: vec![NetworkRule {
            id: "wifi".into(),
            name: "Any Wi-Fi".into(),
            enabled: true,
            conditions: NetworkConditions {
                interface_kind: Some(InterfaceKind::Wifi),
                ..Default::default()
            },
            actions: PROXY_ON,
        }],
    }
}

#[tokio::test]
async fn polls_read_the_wifi_name_only_for_a_rule_that_matches_on_it() {
    let without = Fixture::spawn(wifi_kind_policy(), wifi("Home")).await;
    without.arrive(wifi("Home")).await;
    assert!(
        without
            .environment
            .requests()
            .iter()
            .all(|request| *request == WifiNameRead::Skip)
    );

    let with = Fixture::spawn(policy(), wifi("Home")).await;
    with.arrive(wifi("Home")).await;
    assert!(
        with.environment
            .requests()
            .iter()
            .all(|request| *request == WifiNameRead::Read),
        "a poll reads the name but never asks the user for it"
    );
}

#[tokio::test]
async fn a_policy_that_starts_matching_on_the_name_settles_before_it_applies() {
    let fixture = Fixture::spawn(wifi_kind_policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_ON]);

    // The settled reading holds no name. Evaluating it against the new rule
    // would fall through to the default and write the wrong switches.
    fixture.set_policy(policy()).await;
    assert_eq!(fixture.applied(), [PROXY_ON]);

    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_ON, PROXY_OFF]);
}

#[tokio::test]
async fn requesting_the_wifi_name_asks_without_unsettling_the_network() {
    let fixture = Fixture::spawn(wifi_kind_policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;

    let status = fixture.client.request_wifi_name().await;

    assert_eq!(status.network, Some(wifi("Home")));
    assert_eq!(
        fixture.environment.requests().last(),
        Some(&WifiNameRead::ReadAndAsk)
    );
    assert_eq!(
        fixture.applied(),
        [PROXY_ON],
        "asking for the name writes nothing"
    );

    // The poll that owns the reading is unaffected, so it still agrees with
    // itself and nothing is applied again.
    fixture.client.poll().await;
    assert_eq!(fixture.applied(), [PROXY_ON]);
}

#[tokio::test]
async fn a_refused_wifi_name_is_reported_and_matches_no_name_rule() {
    let refused = NetworkState::Online {
        network: NetworkObservation {
            interface_kind: InterfaceKind::Wifi,
            wifi: WifiName::Unavailable {
                reason: WifiUnavailable::PermissionDenied,
            },
            gateway_mac: None,
        },
    };
    let fixture = Fixture::spawn(policy(), refused.clone()).await;

    fixture.arrive(refused.clone()).await;

    assert_eq!(
        fixture.applied(),
        [PROXY_ON],
        "the default, not the Home rule"
    );
    assert_eq!(fixture.client.status().await.network, Some(refused));
}

#[tokio::test]
async fn a_stopped_actor_reports_an_unsupported_status() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.client.actor.stop(None);
    fixture.client.actor.wait(None).await.unwrap();

    assert_eq!(
        fixture.client.status().await,
        NetworkAutomationStatus::unsupported()
    );
}

#[tokio::test]
async fn capture_switches_commit_system_proxy_through_chimera_owner() {
    let (client, _events) = super::super::tests::recording_client(false).await;
    let switches = super::switches::ConfigCaptureSwitches {
        client: client.clone(),
    };
    assert!(!client.get_app_config().unwrap().enable_system_proxy);

    let first = switches.apply(PROXY_ON).await;
    assert!(
        matches!(
            first.system_proxy,
            MechanismOutcome::Enabled | MechanismOutcome::Failed { .. }
        ),
        "the source may commit with a legacy mirror degradation in the test fixture"
    );
    assert!(
        client.get_app_config().unwrap().enable_system_proxy,
        "a network action must use the same typed source as manual settings"
    );

    let unchanged = switches.apply(PROXY_ON).await;
    assert_eq!(unchanged.system_proxy, MechanismOutcome::Unchanged);

    let second = switches.apply(PROXY_OFF).await;
    assert!(matches!(
        second.system_proxy,
        MechanismOutcome::Disabled | MechanismOutcome::Failed { .. }
    ));
    assert!(!client.get_app_config().unwrap().enable_system_proxy);
}

#[tokio::test]
async fn unsupported_transparent_capture_does_not_commit_or_disable_tun() {
    let (client, events) = super::super::tests::recording_client(false).await;
    let switches = super::switches::ConfigCaptureSwitches {
        client: client.clone(),
    };
    let before = client.get_clash_config().unwrap();

    let result = switches
        .apply(MechanismActions {
            transparent_proxy: MechanismAction::Enable,
            ..Default::default()
        })
        .await;
    assert!(matches!(
        result.transparent_proxy,
        MechanismOutcome::Failed { .. }
    ));
    assert_eq!(result.tun, MechanismOutcome::Unchanged);
    let after = client.get_clash_config().unwrap();
    assert_eq!(after.enable_tun_mode, before.enable_tun_mode);
    assert_eq!(after.transparent_proxy, before.transparent_proxy);
    assert_eq!(after.tproxy_port, before.tproxy_port);
    assert!(
        !events.lock().unwrap().contains(&"rebuild"),
        "unsupported capture must not ask the Core owner to apply changes"
    );
}

#[tokio::test]
async fn keep_capture_actions_do_not_touch_config_owners() {
    let (client, events) = super::super::tests::recording_client(false).await;
    let switches = super::switches::ConfigCaptureSwitches { client };
    let result = switches.apply(MechanismActions::default()).await;
    assert_eq!(result.system_proxy, MechanismOutcome::Unchanged);
    assert_eq!(result.tun, MechanismOutcome::Unchanged);
    assert_eq!(result.transparent_proxy, MechanismOutcome::Unchanged);
    assert!(!events.lock().unwrap().contains(&"rebuild"));
}

//! Desktop capture mutation integration tests.
use super::ports::CaptureSwitches;
use chimera_config::application::{MechanismAction, MechanismActions};
use chimera_contracts::network_automation::MechanismOutcome;

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

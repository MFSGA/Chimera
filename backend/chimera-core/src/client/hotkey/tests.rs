//! Hotkey parsing and owner behavior against a recording registrar.
//!
//! These tests exercise the domain and actor without registering shortcuts on
//! the host or changing its keyboard state.
//! Contract: `docs/testing/contracts/hotkey-bindings.md`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use super::{
    HotkeyArgs, HotkeyClient,
    ports::{
        AcceleratorValidator, HotkeyAction, HotkeyActionSink, HotkeyBindings, HotkeyOp,
        HotkeyParseError, ShortcutRegistrar,
    },
};
use crate::client::effects::status::{EffectHealth, EffectRevision, EffectStatus};

fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

struct AnyAccelerator;

impl AcceleratorValidator for AnyAccelerator {
    fn validate(&self, _accelerator: &str) -> Result<(), HotkeyParseError> {
        Ok(())
    }

    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError> {
        Ok(accelerator.to_owned())
    }
}

#[test]
fn parse_accepts_the_persisted_action_comma_accelerator_format() {
    let bindings = HotkeyBindings::parse(
        &entries(&[
            "open_or_close_dashboard,Control+Q",
            " toggle_tun_mode , Control+Shift+T ",
        ]),
        &AnyAccelerator,
    )
    .expect("well-formed persisted bindings should parse");

    assert_eq!(
        bindings.as_map().get("Control+Q"),
        Some(&HotkeyAction::OpenOrCloseDashboard)
    );
    assert_eq!(
        bindings.as_map().get("Control+Shift+T"),
        Some(&HotkeyAction::ToggleTunMode),
        "surrounding whitespace is not part of the accelerator"
    );
}

#[test]
fn parse_rejects_malformed_unknown_empty_segment_and_unmodified_bindings() {
    assert!(matches!(
        HotkeyBindings::parse(&entries(&["open_or_close_dashboard"]), &AnyAccelerator),
        Err(HotkeyParseError::MalformedEntry { .. })
    ));
    assert!(matches!(
        HotkeyBindings::parse(&entries(&["make_coffee,Control+Q"]), &AnyAccelerator),
        Err(HotkeyParseError::UnknownFunction { .. })
    ));
    assert!(matches!(
        HotkeyBindings::parse(&entries(&["toggle_tun_mode,Control++"]), &AnyAccelerator),
        Err(HotkeyParseError::EmptyKeySegment { .. })
    ));
    assert!(matches!(
        HotkeyBindings::parse(&entries(&["toggle_tun_mode,Q"]), &AnyAccelerator),
        Err(HotkeyParseError::MissingSuperKey { .. })
    ));
}

#[test]
fn transparent_capture_shortcuts_are_available_only_on_linux() {
    for action in [
        "toggle_transparent_proxy",
        "enable_transparent_proxy",
        "disable_transparent_proxy",
    ] {
        let binding = format!("{action},Control+Q");
        let result = HotkeyBindings::parse(&[binding], &AnyAccelerator);
        assert_eq!(result.is_ok(), cfg!(target_os = "linux"));
    }
}

#[test]
fn parser_rejects_duplicate_canonical_accelerators() {
    let error = HotkeyBindings::parse(
        &entries(&["enable_tun_mode,Control+Q", "disable_tun_mode,Control+Q"]),
        &AnyAccelerator,
    )
    .expect_err("one global accelerator cannot run two actions");
    assert!(matches!(
        error,
        HotkeyParseError::DuplicateAccelerator { .. }
    ));
}

#[test]
fn diff_emits_only_unbind_rebind_and_bind_operations() {
    let before = HotkeyBindings::from_pairs([
        ("Control+A", HotkeyAction::EnableTunMode),
        ("Control+B", HotkeyAction::EnableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
    ]);
    let after = HotkeyBindings::from_pairs([
        ("Control+B", HotkeyAction::DisableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
        ("Control+D", HotkeyAction::ToggleTunMode),
    ]);

    assert_eq!(
        before.diff(&after),
        vec![
            HotkeyOp::Unbind {
                accelerator: "Control+A".to_owned(),
            },
            HotkeyOp::Rebind {
                accelerator: "Control+B".to_owned(),
                action: HotkeyAction::DisableSystemProxy,
            },
            HotkeyOp::Bind {
                accelerator: "Control+D".to_owned(),
                action: HotkeyAction::ToggleTunMode,
            },
        ]
    );
}

#[derive(Default)]
struct RecordingRegistrar {
    calls: Mutex<Vec<String>>,
    invalid: Mutex<BTreeSet<String>>,
    register_failures: Mutex<BTreeSet<String>>,
    registered: Mutex<BTreeMap<String, HotkeyAction>>,
    last_sink: Mutex<Option<(HotkeyAction, Arc<dyn HotkeyActionSink>)>>,
}

impl RecordingRegistrar {
    async fn client(self: &Arc<Self>, sink: Arc<dyn HotkeyActionSink>) -> HotkeyClient {
        HotkeyClient::spawn(HotkeyArgs {
            registrar: self.clone(),
            sink,
        })
        .await
        .expect("the hotkey actor should start")
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("registrar call log").clone()
    }

    fn clear_calls(&self) {
        self.calls.lock().expect("registrar call log").clear();
    }

    fn set_register_failure(&self, accelerator: &str, fail: bool) {
        let mut failures = self.register_failures.lock().expect("failure set");
        if fail {
            failures.insert(accelerator.to_owned());
        } else {
            failures.remove(accelerator);
        }
    }

    fn fire_last_shortcut(&self) {
        let (action, sink) = self
            .last_sink
            .lock()
            .expect("last sink")
            .clone()
            .expect("a shortcut was registered");
        sink.dispatch(action);
    }
}

#[async_trait::async_trait]
impl ShortcutRegistrar for RecordingRegistrar {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        self.calls
            .lock()
            .expect("registrar call log")
            .push(format!("validate:{accelerator}"));
        if self
            .invalid
            .lock()
            .expect("invalid set")
            .contains(accelerator)
        {
            return Err(HotkeyParseError::UnsupportedAccelerator {
                accelerator: accelerator.to_owned(),
                source: "test registrar rejected accelerator".into(),
            });
        }
        Ok(())
    }

    async fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> anyhow::Result<()> {
        self.calls
            .lock()
            .expect("registrar call log")
            .push(format!("register:{accelerator}"));
        if self
            .register_failures
            .lock()
            .expect("failure set")
            .contains(accelerator)
        {
            anyhow::bail!("the test registrar refused {accelerator}");
        }
        self.registered
            .lock()
            .expect("registered set")
            .insert(accelerator.to_owned(), action);
        *self.last_sink.lock().expect("last sink") = Some((action, sink));
        Ok(())
    }

    async fn unregister(&self, accelerator: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .expect("registrar call log")
            .push(format!("unregister:{accelerator}"));
        self.registered
            .lock()
            .expect("registered set")
            .remove(accelerator);
        Ok(())
    }

    async fn unregister_all(&self) -> anyhow::Result<()> {
        self.calls
            .lock()
            .expect("registrar call log")
            .push("unregister_all".to_owned());
        self.registered.lock().expect("registered set").clear();
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSink(Mutex<Vec<HotkeyAction>>);

impl HotkeyActionSink for RecordingSink {
    fn dispatch(&self, action: HotkeyAction) {
        self.0.lock().expect("action list").push(action);
    }
}

fn bindings(raw: &[&str]) -> HotkeyBindings {
    HotkeyBindings::parse(&entries(raw), &AnyAccelerator).expect("test bindings should parse")
}

fn effect_code(status: &EffectStatus) -> Option<&str> {
    match &status.health {
        EffectHealth::Degraded { code, .. } => Some(code),
        _ => None,
    }
}

#[tokio::test]
async fn actor_releases_every_changed_accelerator_before_registering() {
    let registrar = Arc::new(RecordingRegistrar::default());
    let client = registrar.client(Arc::new(RecordingSink::default())).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A", "enable_system_proxy,Control+B"]),
        )
        .await;
    registrar.clear_calls();

    let status = client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_tun_mode,Control+B", "enable_system_proxy,Control+A"]),
        )
        .await;

    assert_eq!(status.health, EffectHealth::Healthy);
    let calls = registrar.calls();
    let first_register = calls
        .iter()
        .position(|call| call.starts_with("register:"))
        .expect("changed bindings are registered");
    let last_unregister = calls
        .iter()
        .rposition(|call| call.starts_with("unregister:"))
        .expect("changed bindings are released");
    assert!(last_unregister < first_register, "calls were {calls:?}");
}

#[tokio::test]
async fn invalid_desired_binding_does_not_release_a_working_binding() {
    let registrar = Arc::new(RecordingRegistrar::default());
    let client = registrar.client(Arc::new(RecordingSink::default())).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.clear_calls();
    registrar
        .invalid
        .lock()
        .expect("invalid set")
        .insert("Control+B".to_owned());
    let desired = HotkeyBindings::from_pairs([
        ("Control+A", HotkeyAction::EnableTunMode),
        ("Control+B", HotkeyAction::EnableSystemProxy),
    ]);

    let status = client.reconcile(EffectRevision::new(2), desired).await;

    assert_eq!(effect_code(&status), Some("hotkey_invalid_bindings"));
    assert_eq!(
        registrar
            .registered
            .lock()
            .expect("registered set")
            .get("Control+A"),
        Some(&HotkeyAction::EnableTunMode)
    );
    assert!(
        registrar
            .calls()
            .iter()
            .all(|call| call.starts_with("validate:")),
        "validation must finish before any OS registration changes"
    );
}

#[tokio::test]
async fn partial_registration_keeps_successes_and_retries_only_missing_bindings() {
    let registrar = Arc::new(RecordingRegistrar::default());
    registrar.set_register_failure("Control+B", true);
    let client = registrar.client(Arc::new(RecordingSink::default())).await;
    let desired = bindings(&["enable_tun_mode,Control+A", "enable_system_proxy,Control+B"]);

    let first = client
        .reconcile(EffectRevision::new(1), desired.clone())
        .await;

    assert_eq!(effect_code(&first), Some("hotkey_partial_registration"));
    assert_eq!(client.status().await.registered.len(), 1);
    registrar.set_register_failure("Control+B", false);
    registrar.clear_calls();

    let recovered = client.reconcile(EffectRevision::new(2), desired).await;

    assert_eq!(recovered.health, EffectHealth::Healthy);
    assert_eq!(client.status().await.registered.len(), 2);
    let registration_calls: Vec<_> = registrar
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("register:"))
        .collect();
    assert_eq!(registration_calls, ["register:Control+B"]);
}

#[tokio::test]
async fn stale_revision_is_superseded_without_touching_the_registrar() {
    let registrar = Arc::new(RecordingRegistrar::default());
    let client = registrar.client(Arc::new(RecordingSink::default())).await;
    client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.clear_calls();

    let stale = client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_system_proxy,Control+B"]),
        )
        .await;

    assert_eq!(stale.health, EffectHealth::Superseded);
    assert!(registrar.calls().is_empty());
    assert_eq!(
        client.status().await.applied_revision,
        EffectRevision::new(2)
    );
}

#[tokio::test]
async fn unregister_all_closes_the_owner_against_late_reconciles() {
    let registrar = Arc::new(RecordingRegistrar::default());
    let client = registrar.client(Arc::new(RecordingSink::default())).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    assert_eq!(client.unregister_all().await.health, EffectHealth::Healthy);
    registrar.clear_calls();

    let late = client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_system_proxy,Control+B"]),
        )
        .await;

    assert_eq!(effect_code(&late), Some("hotkey_shut_down"));
    assert!(registrar.calls().is_empty());
}

#[tokio::test]
async fn registered_callback_dispatches_only_its_action_to_the_sink() {
    let registrar = Arc::new(RecordingRegistrar::default());
    let sink = Arc::new(RecordingSink::default());
    let client = registrar.client(sink.clone()).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["toggle_tun_mode,Control+T"]),
        )
        .await;

    registrar.fire_last_shortcut();

    assert_eq!(
        *sink.0.lock().expect("action list"),
        [HotkeyAction::ToggleTunMode]
    );
}

#[test]
fn transparent_capture_switch_preserves_mode_and_existing_listener() {
    use chimera_config::clash::config::{
        ClashConfig, TransparentProxyMode,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    };
    let mut config = ClashConfig::default();
    config.tproxy_port = None;
    let patch = super::switch_patch(&config, true);
    assert_eq!(
        patch.transparent_proxy.mode,
        Some(TransparentProxyMode::Tproxy)
    );
    assert_eq!(
        patch.tproxy_port,
        Some(Some(PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: 7894
        }))
    );
    config.tproxy_port = Some(PortStrategy::new_allow_fallback(7895));
    assert!(super::switch_patch(&config, true).tproxy_port.is_none());
    config.transparent_proxy.mode = TransparentProxyMode::Redir;
    let keep = super::switch_patch(&config, true);
    assert_eq!(
        keep.transparent_proxy.mode,
        Some(TransparentProxyMode::Redir)
    );
    assert!(keep.tproxy_port.is_none());
    let disable = super::switch_patch(&config, false);
    assert_eq!(
        disable.transparent_proxy.mode,
        Some(TransparentProxyMode::Disabled)
    );
    assert!(disable.tproxy_port.is_none());
}

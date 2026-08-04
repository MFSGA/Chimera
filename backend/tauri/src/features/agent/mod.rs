mod actions;
mod actor;
mod adapters;
mod bridge;
mod client;
pub(crate) mod commands;
mod core_probe;
mod diagnostics;
mod error;
mod history;
mod intent;
mod model;
mod planning;
mod ports;
mod registry;

pub(crate) use adapters::{
    FsAgentHistoryPersistence, HttpAgentBridge, HttpBridgeHealth, HttpNetworkProbe,
    LegacyAgentConfiguration, LegacyAgentMutation, LegacyAgentRuntime, LegacyCoreLifecycle,
    LegacyCoreRoutingProbe, LegacyServiceControl, LegacySystemProxy, RegistryAgentToolExecutor,
    TauriAgentConfirmation, TauriAgentTelemetry,
};
pub(crate) use bridge::{AgentBridgeStartResult, AgentBridgeStatus};
pub(crate) use client::AgentClient;
pub(crate) use history::AgentHistorySnapshot;
pub(crate) use intent::resolve_intent;
pub(crate) use model::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentIntentRequest,
    AgentIntentResolution, AgentManifest, AgentNetworkProbeRequest, AgentNetworkSnapshot,
    AgentProposal, AgentToolManifest, AgentToolName, AgentToolRisk,
};
pub(crate) use registry::agent_manifest;

#[cfg(test)]
mod capability_boundary_tests {
    const PRODUCTION_SOURCES: [(&str, &str); 34] = [
        ("actions.rs", include_str!("actions.rs")),
        ("actor.rs", include_str!("actor.rs")),
        ("bridge.rs", include_str!("bridge.rs")),
        ("client.rs", include_str!("client.rs")),
        ("commands.rs", include_str!("commands.rs")),
        ("core_probe.rs", include_str!("core_probe.rs")),
        ("diagnostics.rs", include_str!("diagnostics.rs")),
        ("error.rs", include_str!("error.rs")),
        ("history.rs", include_str!("history.rs")),
        ("intent.rs", include_str!("intent.rs")),
        ("model.rs", include_str!("model.rs")),
        ("planning.rs", include_str!("planning.rs")),
        ("ports.rs", include_str!("ports.rs")),
        ("registry.rs", include_str!("registry.rs")),
        (
            "registry_execution.rs",
            include_str!("registry/execution.rs"),
        ),
        ("registry_manifest.rs", include_str!("registry/manifest.rs")),
        ("registry_output.rs", include_str!("registry/output.rs")),
        ("registry_probe.rs", include_str!("registry/probe.rs")),
        ("registry_request.rs", include_str!("registry/request.rs")),
        ("fs_history.rs", include_str!("adapters/fs_history.rs")),
        ("http_bridge.rs", include_str!("adapters/http_bridge.rs")),
        (
            "http_bridge_health.rs",
            include_str!("adapters/http_bridge_health.rs"),
        ),
        (
            "http_network_probe.rs",
            include_str!("adapters/http_network_probe.rs"),
        ),
        (
            "legacy_config.rs",
            include_str!("adapters/legacy_config.rs"),
        ),
        ("legacy_core.rs", include_str!("adapters/legacy_core.rs")),
        (
            "legacy_mutation.rs",
            include_str!("adapters/legacy_mutation.rs"),
        ),
        (
            "legacy_routing_probe.rs",
            include_str!("adapters/legacy_routing_probe.rs"),
        ),
        (
            "legacy_runtime.rs",
            include_str!("adapters/legacy_runtime.rs"),
        ),
        (
            "legacy_service.rs",
            include_str!("adapters/legacy_service.rs"),
        ),
        (
            "legacy_snapshot.rs",
            include_str!("adapters/legacy_snapshot.rs"),
        ),
        (
            "legacy_system_proxy.rs",
            include_str!("adapters/legacy_system_proxy.rs"),
        ),
        (
            "tauri_confirmation.rs",
            include_str!("adapters/tauri_confirmation.rs"),
        ),
        (
            "tauri_telemetry.rs",
            include_str!("adapters/tauri_telemetry.rs"),
        ),
        (
            "tool_executor.rs",
            include_str!("adapters/tool_executor.rs"),
        ),
    ];

    const NON_FILESYSTEM_PRODUCTION_SOURCES: [(&str, &str); 33] = [
        ("actions.rs", include_str!("actions.rs")),
        ("actor.rs", include_str!("actor.rs")),
        ("bridge.rs", include_str!("bridge.rs")),
        ("client.rs", include_str!("client.rs")),
        ("commands.rs", include_str!("commands.rs")),
        ("core_probe.rs", include_str!("core_probe.rs")),
        ("diagnostics.rs", include_str!("diagnostics.rs")),
        ("error.rs", include_str!("error.rs")),
        ("history.rs", include_str!("history.rs")),
        ("intent.rs", include_str!("intent.rs")),
        ("model.rs", include_str!("model.rs")),
        ("planning.rs", include_str!("planning.rs")),
        ("ports.rs", include_str!("ports.rs")),
        ("registry.rs", include_str!("registry.rs")),
        (
            "registry_execution.rs",
            include_str!("registry/execution.rs"),
        ),
        ("registry_manifest.rs", include_str!("registry/manifest.rs")),
        ("registry_output.rs", include_str!("registry/output.rs")),
        ("registry_probe.rs", include_str!("registry/probe.rs")),
        ("registry_request.rs", include_str!("registry/request.rs")),
        ("http_bridge.rs", include_str!("adapters/http_bridge.rs")),
        (
            "http_bridge_health.rs",
            include_str!("adapters/http_bridge_health.rs"),
        ),
        (
            "http_network_probe.rs",
            include_str!("adapters/http_network_probe.rs"),
        ),
        (
            "legacy_config.rs",
            include_str!("adapters/legacy_config.rs"),
        ),
        ("legacy_core.rs", include_str!("adapters/legacy_core.rs")),
        (
            "legacy_mutation.rs",
            include_str!("adapters/legacy_mutation.rs"),
        ),
        (
            "legacy_routing_probe.rs",
            include_str!("adapters/legacy_routing_probe.rs"),
        ),
        (
            "legacy_runtime.rs",
            include_str!("adapters/legacy_runtime.rs"),
        ),
        (
            "legacy_service.rs",
            include_str!("adapters/legacy_service.rs"),
        ),
        (
            "legacy_snapshot.rs",
            include_str!("adapters/legacy_snapshot.rs"),
        ),
        (
            "legacy_system_proxy.rs",
            include_str!("adapters/legacy_system_proxy.rs"),
        ),
        (
            "tauri_confirmation.rs",
            include_str!("adapters/tauri_confirmation.rs"),
        ),
        (
            "tauri_telemetry.rs",
            include_str!("adapters/tauri_telemetry.rs"),
        ),
        (
            "tool_executor.rs",
            include_str!("adapters/tool_executor.rs"),
        ),
    ];

    fn production_only(source: &str) -> &str {
        let before_inline_tests = source.split("#[cfg(test)]\nmod ").next().unwrap_or(source);
        before_inline_tests
            .split("#[cfg(test)]\n#[path")
            .next()
            .unwrap_or(before_inline_tests)
    }

    #[test]
    fn agent_production_code_cannot_spawn_processes_or_shells() {
        let forbidden = [
            ["std", "::process"].concat(),
            ["tokio", "::process"].concat(),
            ["Command", "::new"].concat(),
            ["power", "shell"].concat(),
            ["cmd", ".exe"].concat(),
            ["/bin/", "sh"].concat(),
            ["tauri_plugin_", "shell"].concat(),
        ];

        for (name, source) in PRODUCTION_SOURCES {
            let source = production_only(source);
            for capability in &forbidden {
                assert!(
                    !source.contains(capability),
                    "{name} must not introduce process or shell capability: {capability}"
                );
            }
        }
    }

    #[test]
    fn filesystem_capability_is_confined_to_the_history_adapter() {
        let forbidden = [
            ["std", "::fs"].concat(),
            ["tokio", "::fs"].concat(),
            ["Open", "Options"].concat(),
            ["File", "::open"].concat(),
            ["File", "::create"].concat(),
            ["remove", "_file"].concat(),
            ["create", "_dir"].concat(),
            ["read", "_to_string"].concat(),
        ];

        for (name, source) in NON_FILESYSTEM_PRODUCTION_SOURCES {
            let source = production_only(source);
            for capability in &forbidden {
                assert!(
                    !source.contains(capability),
                    "{name} must not introduce filesystem capability: {capability}"
                );
            }
        }
    }

    #[test]
    fn agent_production_code_avoids_assertion_driven_panics() {
        let forbidden = [
            ".unwrap(",
            ".expect(",
            "panic!(",
            "todo!(",
            "unimplemented!(",
        ];

        for (name, source) in PRODUCTION_SOURCES {
            let source = production_only(source);
            for construct in forbidden {
                assert!(
                    !source.contains(construct),
                    "{name} must return or project failures instead of using {construct}"
                );
            }
        }
    }

    #[test]
    fn long_lived_agent_state_is_split_across_typed_actors() {
        let actors = production_only(include_str!("actor.rs"));
        let client = production_only(include_str!("client.rs"));
        let actions = production_only(include_str!("actions.rs"));
        let history = production_only(include_str!("history.rs"));
        let bridge = production_only(include_str!("bridge.rs"));

        for actor in [
            "AgentProposalActor",
            "AgentHistoryActor",
            "AgentBridgeActor",
        ] {
            assert!(actors.contains(actor), "missing typed actor: {actor}");
        }
        for client_field in [
            "proposals: AgentProposalClient",
            "history: AgentHistoryClient",
            "bridge: AgentBridgeClient",
        ] {
            assert!(
                client.contains(client_field),
                "missing typed actor client: {client_field}"
            );
        }
        assert!(client.contains("impl<Message> Drop for AgentActorHandle<Message>"));
        assert!(client.contains("self.actor.stop(None);"));
        assert!(!client.contains("Some(\"agent-"));
        assert!(!client.contains("ActorRef::where_is"));
        for actor_client in [
            "Arc<AgentActorHandle<AgentProposalMessage>>",
            "Arc<AgentActorHandle<AgentHistoryMessage>>",
            "Arc<AgentActorHandle<AgentBridgeMessage>>",
        ] {
            assert!(
                client.contains(actor_client),
                "typed client must share its actor lifecycle handle: {actor_client}"
            );
        }
        let bridge_wrapped = client
            .find("let bridge_client = AgentBridgeClient::new(bridge);")
            .expect("Bridge actor must be wrapped immediately after spawn");
        let proposal_spawn = client
            .find("let proposals = Actor::spawn(")
            .expect("proposal actor spawn");
        assert!(bridge_wrapped < proposal_spawn);
        assert!(!actions.contains("Mutex<ProposalStore>"));
        assert!(!history.contains("Mutex<Option<AgentHistoryDocument>>"));
        assert!(!bridge.contains("Mutex<Option<BridgeRuntime>>"));
    }

    #[test]
    fn agent_composition_root_is_available_on_every_supported_platform() {
        let lib = include_str!("../../lib.rs");
        let facade = include_str!("../../client.rs");
        assert!(lib.contains("#[cfg(feature = \"agent\")]\nmod setup;"));
        assert!(lib.contains("#[cfg(feature = \"agent\")]\n            setup::setup(app)?;"));
        assert!(!lib.contains("#[cfg(windows)]\n#[cfg(feature = \"agent\")]\nmod setup;"));
        assert!(facade.contains("struct NyanpasuClientInner"));
        assert!(facade.contains("inner: Arc<NyanpasuClientInner>"));
        assert!(facade.contains("Arc::new(NyanpasuClientInner { agent })"));

        let workflow = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.github/workflows/agent-ci.yaml"
        ));
        for composition_file in [
            "backend/tauri/src/client.rs",
            "backend/tauri/src/core/clash/core.rs",
            "backend/tauri/src/feat.rs",
            "backend/tauri/src/setup.rs",
        ] {
            assert_eq!(
                workflow.matches(composition_file).count(),
                2,
                "Agent CI must run for pull requests and pushes that change {composition_file}"
            );
        }
    }

    #[test]
    fn idempotent_core_start_is_serialized_by_the_core_lifecycle_lock() {
        let core = include_str!("../../core/clash/core.rs");
        let start = core
            .find("pub async fn ensure_core_running(&self) -> Result<()> {")
            .expect("idempotent core start method");
        let end = core[start..]
            .find("\n    async fn run_core_inner")
            .map(|offset| start + offset)
            .expect("end of idempotent core start method");
        let method = &core[start..end];

        let lock = method
            .find("self.run_lock.lock().await")
            .expect("core lifecycle lock");
        let state_check = method
            .find("instance.state().await")
            .expect("core state check");
        let start_inner = method
            .find("self.run_core_inner().await")
            .expect("core start implementation");
        assert!(lock < state_check && state_check < start_inner);
        assert!(!method.contains("self.run_core().await"));
    }

    #[test]
    fn core_lifecycle_global_is_confined_to_the_legacy_adapter() {
        let legacy_core = include_str!("adapters/legacy_core.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let snapshot = include_str!("adapters/legacy_snapshot.rs");
        let setup = include_str!("../../setup.rs");

        assert!(legacy_core.contains("CoreManager::global()"));
        assert!(legacy_core.contains("impl CoreLifecyclePort for LegacyCoreLifecycle"));
        assert!(!runtime.contains("CoreManager::global()"));
        assert!(!snapshot.contains("CoreManager::global()"));
        assert!(runtime.contains("core: Arc<dyn CoreLifecyclePort>"));
        assert!(snapshot.contains("core_lifecycle: &dyn CoreLifecyclePort"));
        assert!(setup.contains("LegacyCoreLifecycle::new()"));
        assert!(setup.contains("LegacyAgentRuntime::new("));
        assert!(setup.contains("configuration,"));
    }

    #[test]
    fn service_lifecycle_module_state_is_confined_to_the_legacy_adapter() {
        let legacy_service = include_str!("adapters/legacy_service.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let snapshot = include_str!("adapters/legacy_snapshot.rs");
        let diagnostics = include_str!("diagnostics.rs");
        let setup = include_str!("../../setup.rs");

        assert!(legacy_service.contains("impl ServiceControlPort for LegacyServiceControl"));
        assert!(legacy_service.contains("service::control::status()"));
        assert!(legacy_service.contains("service::ipc::get_ipc_state()"));
        for source in [runtime, snapshot, diagnostics] {
            assert!(!source.contains("service::control"));
            assert!(!source.contains("service::ipc"));
        }
        assert!(runtime.contains("service: Arc<dyn ServiceControlPort>"));
        assert!(snapshot.contains("service_control: &dyn ServiceControlPort"));
        assert!(setup.contains("LegacyServiceControl::new()"));
        assert!(setup.contains("LegacyAgentRuntime::new("));
        assert!(setup.contains("configuration,"));
    }

    #[test]
    fn tauri_telemetry_capability_is_confined_to_the_adapter() {
        let telemetry = include_str!("adapters/tauri_telemetry.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let snapshot = include_str!("adapters/legacy_snapshot.rs");
        let setup = include_str!("../../setup.rs");

        assert!(telemetry.contains("app: AppHandle"));
        assert!(telemetry.contains("try_state::<ClashConnectionsConnector>()"));
        assert!(telemetry.contains("restart_ws_connector(&self.app)"));
        assert!(!runtime.contains("AppHandle"));
        assert!(!snapshot.contains("AppHandle"));
        assert!(!snapshot.contains("ClashConnectionsConnector"));
        assert!(runtime.contains("telemetry: Arc<dyn AgentTelemetryPort>"));
        assert!(snapshot.contains("telemetry_port: &dyn AgentTelemetryPort"));
        assert!(setup.contains("TauriAgentTelemetry::new(app_handle.clone())"));
        assert!(setup.contains("LegacyAgentRuntime::new("));
        assert!(setup.contains("configuration,"));
    }

    #[test]
    fn configuration_globals_are_confined_to_the_legacy_adapter() {
        let configuration = include_str!("adapters/legacy_config.rs");
        let routing_probe = include_str!("adapters/legacy_routing_probe.rs");
        let core_probe = include_str!("core_probe.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let snapshot = include_str!("adapters/legacy_snapshot.rs");
        let setup = include_str!("../../setup.rs");

        for global in [
            "Config::verge()",
            "Config::clash()",
            "Config::runtime()",
            "Config::profiles()",
        ] {
            assert!(configuration.contains(global));
            assert!(!snapshot.contains(global));
        }
        assert!(configuration.contains("impl AgentConfigurationPort for LegacyAgentConfiguration"));
        assert!(routing_probe.contains("Config::clash()"));
        assert!(routing_probe.contains("impl CoreRoutingProbePort for LegacyCoreRoutingProbe"));
        assert!(!core_probe.contains("Config::"));
        assert!(runtime.contains("configuration: Arc<dyn AgentConfigurationPort>"));
        assert!(runtime.contains("routing_probe: Arc<dyn CoreRoutingProbePort>"));
        assert!(snapshot.contains("configuration_port: &dyn AgentConfigurationPort"));
        assert!(snapshot.contains("routing_probe: &dyn CoreRoutingProbePort"));
        assert!(setup.contains("LegacyAgentConfiguration::new()"));
        assert!(setup.contains("LegacyCoreRoutingProbe::new()"));
        assert!(setup.contains("LegacyAgentRuntime::new("));
    }

    #[test]
    fn controller_probe_network_io_is_confined_to_the_legacy_adapter() {
        let adapter = include_str!("adapters/legacy_routing_probe.rs");
        let core_probe = include_str!("core_probe.rs");

        assert!(adapter.contains("reqwest::ClientBuilder::new()"));
        assert!(adapter.contains(".no_proxy()"));
        assert!(adapter.contains(".redirect(Policy::none())"));
        assert!(adapter.contains("request.bearer_auth(secret)"));
        assert!(core_probe.contains("loopback_controller_url"));
        assert!(!core_probe.contains("reqwest::"));
        assert!(!core_probe.contains("Deserialize"));
        assert!(!core_probe.contains("bearer_auth"));
    }

    #[test]
    fn runtime_mutations_are_confined_to_the_legacy_adapter() {
        let mutation = include_str!("adapters/legacy_mutation.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let setup = include_str!("../../setup.rs");

        for operation in [
            "feat::set_tun_enabled",
            "feat::set_system_proxy_enabled",
            "feat::patch_verge",
            "feat::set_service_mode",
            "feat::restore_service_mode",
            "feat::set_routing_mode",
        ] {
            assert!(mutation.contains(operation));
            assert!(!runtime.contains(operation));
        }
        assert!(mutation.contains("impl AgentMutationPort for LegacyAgentMutation"));
        assert!(runtime.contains("mutation: Arc<dyn AgentMutationPort>"));
        assert!(setup.contains("LegacyAgentMutation::new()"));
    }

    #[test]
    fn public_model_does_not_depend_on_legacy_config_types() {
        let model = production_only(include_str!("model.rs"));
        let configuration = include_str!("adapters/legacy_config.rs");

        assert!(!model.contains("crate::config"));
        assert!(!model.contains("ClashCore"));
        assert!(configuration.contains("fn map_selected_core("));
        assert!(configuration.contains("ClashCore::ClashPremium"));
    }

    #[test]
    fn diagnostics_depends_only_on_agent_domain_projections() {
        let diagnostics = production_only(include_str!("diagnostics.rs"));
        let configuration = include_str!("adapters/legacy_config.rs");
        let core = include_str!("adapters/legacy_core.rs");

        for forbidden in [
            "crate::config",
            "chimera_ipc",
            "serde_yaml::Mapping",
            "sysproxy::",
            "ProfileMetaGetter",
            "CoreManager",
        ] {
            assert!(
                !diagnostics.contains(forbidden),
                "diagnostics leaked {forbidden}"
            );
        }
        assert!(configuration.contains("fn generated_tun_enabled("));
        assert!(configuration.contains("fn summarize_profiles("));
        assert!(core.contains("fn map_core_state("));
        assert!(core.contains("fn map_run_type("));
    }

    #[test]
    fn service_mode_rollback_forces_runtime_reapplication() {
        let feat = include_str!("../../feat.rs");
        let mutation = include_str!("adapters/legacy_mutation.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");

        let restore_start = feat
            .find("pub(crate) async fn restore_service_mode(enabled: bool)")
            .expect("forced service-mode restore entry point");
        let restore_end = feat[restore_start..]
            .find("async fn apply_service_mode")
            .map(|offset| restore_start + offset)
            .expect("service-mode apply helper");
        let restore = &feat[restore_start..restore_end];
        assert!(restore.contains("apply_service_mode(enabled).await"));
        assert!(!restore.contains("current == enabled"));

        let rollback_start = runtime
            .find("async fn rollback_service_mode(&self, target: bool)")
            .expect("service-mode rollback helper");
        let rollback_end = runtime[rollback_start..]
            .find("async fn rollback_routing_mode")
            .map(|offset| rollback_start + offset)
            .expect("end of service-mode rollback helper");
        let rollback = &runtime[rollback_start..rollback_end];
        assert!(mutation.contains("feat::restore_service_mode(enabled)"));
        assert!(rollback.contains("self.mutation.restore_service_mode(target)"));
        assert!(!rollback.contains("set_service_mode(target)"));
    }

    #[test]
    fn history_blocking_io_is_bounded_and_single_flight() {
        let persistence = include_str!("adapters/fs_history.rs");
        assert!(persistence.contains("const HISTORY_BLOCKING_IO_TIMEOUT"));
        assert!(persistence.contains("blocking_io: HistoryBlockingIo"));
        assert!(persistence.contains("tokio::sync::Semaphore::new(1)"));

        let acquire = persistence
            .find("self.gate.clone().acquire_owned()")
            .expect("history I/O gate acquisition");
        let blocking = persistence
            .find("tokio::task::spawn_blocking(move || {")
            .expect("history blocking task");
        let permit = persistence
            .find("let _permit = permit;")
            .expect("permit retained by history blocking task");
        assert!(acquire < blocking && blocking < permit);
        assert!(persistence.contains("read_document_from_with_io(&self.path, &self.blocking_io)"));
        assert!(
            persistence
                .contains("write_document_to_with_io(&self.path, document, &self.blocking_io)")
        );
    }

    #[test]
    fn blocking_system_proxy_io_is_bounded_and_single_flight() {
        let adapter = include_str!("adapters/legacy_system_proxy.rs");
        let runtime = include_str!("adapters/legacy_runtime.rs");
        let snapshot = include_str!("adapters/legacy_snapshot.rs");
        let diagnostics = include_str!("diagnostics.rs");
        let setup = include_str!("../../setup.rs");

        assert!(adapter.contains("gate: Arc<tokio::sync::Semaphore>"));
        assert!(adapter.contains("tokio::sync::Semaphore::new(1)"));
        assert!(adapter.contains("impl SystemProxyPort for LegacySystemProxy"));
        assert!(adapter.contains("Sysproxy::get_system_proxy()"));
        assert!(adapter.contains("set_system_proxy()"));
        for source in [runtime, snapshot, diagnostics] {
            assert!(!source.contains("Sysproxy"));
        }
        assert!(runtime.contains("system_proxy: Arc<dyn SystemProxyPort>"));
        assert!(snapshot.contains("system_proxy_port: &dyn SystemProxyPort"));
        assert!(snapshot.contains("system_proxy_port.probe()"));
        assert!(diagnostics.contains("Option<SystemProxyConfiguration>"));
        assert!(setup.contains("LegacySystemProxy::new(mutation.clone())"));

        let probe_start = adapter.find("async fn probe(&self)").expect("probe entry");
        let read_start = adapter[probe_start..]
            .find("async fn read(&self)")
            .map(|offset| probe_start + offset)
            .expect("read entry");
        let probe = &adapter[probe_start..read_start];
        let probe_acquire = probe
            .find("self.gate.clone().try_acquire_owned().ok()?")
            .expect("non-blocking probe gate");
        let probe_blocking = probe
            .find("tokio::task::spawn_blocking(move || {")
            .expect("blocking system proxy probe");
        let probe_permit = probe
            .find("let _permit = permit;")
            .expect("probe retains permit");
        assert!(probe_acquire < probe_blocking && probe_blocking < probe_permit);

        let write_start = adapter[read_start..]
            .find("async fn write(&self")
            .map(|offset| read_start + offset)
            .expect("write entry");
        let read = &adapter[read_start..write_start];
        let read_acquire = read
            .find("self.gate.clone().acquire_owned()")
            .expect("owned read gate");
        let read_blocking = read
            .find("tokio::task::spawn_blocking(move || {")
            .expect("blocking system proxy read");
        let read_permit = read
            .find("let _permit = permit;")
            .expect("read retains permit");
        assert!(read_acquire < read_blocking && read_blocking < read_permit);

        let apply_start = adapter
            .find("async fn apply_desired(&self")
            .expect("desired-state entry");
        let apply = &adapter[apply_start..];
        let apply_acquire = apply
            .find("self.gate.clone().acquire_owned()")
            .expect("owned desired-state gate");
        let apply_task = apply
            .find("tokio::spawn(async move {")
            .expect("detached-safe desired-state task");
        let apply_permit = apply
            .find("let _permit = permit;")
            .expect("desired-state task retains permit");
        let apply_change = apply
            .find("mutation.set_system_proxy_enabled(enabled)")
            .expect("desired-state mutation");
        assert!(
            apply_acquire < apply_task && apply_task < apply_permit && apply_permit < apply_change
        );
        assert!(runtime.contains("self.system_proxy.apply_desired(target)"));
        assert!(runtime.contains("self.system_proxy.read()"));
        assert!(runtime.contains("self.system_proxy.write("));
    }

    #[test]
    fn native_confirmation_dialogs_are_single_flight_until_the_blocking_task_finishes() {
        let adapter = include_str!("adapters/tauri_confirmation.rs");
        assert!(adapter.contains("dialog_gate: Arc<tokio::sync::Semaphore>"));
        assert!(adapter.contains("tokio::sync::Semaphore::new(1)"));

        let acquire = adapter
            .find(".acquire_owned()")
            .expect("owned confirmation permit");
        let blocking = adapter
            .find("tokio::task::spawn_blocking(move || {")
            .expect("native dialog blocking task");
        let permit_move = adapter
            .find("let _permit = permit;")
            .expect("permit retained by blocking task");
        assert!(acquire < blocking && blocking < permit_move);
    }

    #[test]
    fn history_clear_confirmation_is_owned_by_the_backend_boundary() {
        let commands = production_only(include_str!("commands.rs"));
        let client = production_only(include_str!("client.rs"));
        let ports = production_only(include_str!("ports.rs"));
        let adapter = production_only(include_str!("adapters/tauri_confirmation.rs"));
        let page = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/chimera/src/features/agent/agent-page.tsx"
        ));

        assert!(
            commands
                .contains("pub(crate) async fn agent_clear_history(\n    window: WebviewWindow,")
        );
        assert!(commands.contains("client.agent_clear_history(window.label()).await"));
        assert!(client.contains("confirmation.confirm_history_clear(owner_label)"));
        assert!(client.contains("AGENT_HISTORY_CONFIRMATION_TIMEOUT"));
        assert!(ports.contains("async fn confirm_history_clear(&self, owner_label: &str)"));
        assert!(adapter.contains("async fn confirm_history_clear(&self, owner_label: &str)"));
        assert!(!page.contains("@tauri-apps/plugin-dialog"));
        assert!(!page.contains("await ask("));
    }

    #[test]
    fn network_probe_infrastructure_is_injected_into_registry_dispatch() {
        let adapter = include_str!("adapters/http_network_probe.rs");
        let execution = include_str!("registry/execution.rs");
        let validation = include_str!("registry/probe.rs");
        let executor = include_str!("adapters/tool_executor.rs");
        let setup = include_str!("../../setup.rs");

        assert!(adapter.contains("impl NetworkProbePort for HttpNetworkProbe"));
        assert!(adapter.contains("lookup_host((domain, port))"));
        assert!(adapter.contains(".no_proxy()"));
        assert!(adapter.contains(".redirect(Policy::none())"));
        for source in [execution, validation] {
            assert!(!source.contains("reqwest::"));
            assert!(!source.contains("lookup_host"));
        }
        assert!(execution.contains("network_probe: &dyn NetworkProbePort"));
        assert!(execution.contains("network_probe.execute(request.arguments)"));
        assert!(executor.contains("network_probe: Arc<dyn NetworkProbePort>"));
        assert!(setup.contains("HttpNetworkProbe::new()"));
        assert!(setup.contains("RegistryAgentToolExecutor::new("));
        assert!(setup.contains("runtime.clone(),"));
        assert!(setup.contains("network_probe,"));
    }

    #[test]
    fn bridge_health_network_io_is_injected_into_the_actor_boundary() {
        let health = include_str!("adapters/http_bridge_health.rs");
        let server = include_str!("adapters/http_bridge.rs");
        let bridge_dto = include_str!("bridge.rs");
        let actor = include_str!("actor.rs");
        let client = include_str!("client.rs");
        let setup = include_str!("../../setup.rs");

        assert!(health.contains("impl AgentBridgeHealthPort for HttpBridgeHealth"));
        assert!(health.contains("reqwest::Client::builder()"));
        assert!(health.contains(".no_proxy()"));
        assert!(health.contains(".redirect(Policy::none())"));
        assert!(health.contains("MAX_HEALTH_RESPONSE_BYTES: usize = 256"));
        assert!(server.contains("impl AgentBridgePort for HttpAgentBridge"));
        assert!(server.contains("health: Arc<dyn AgentBridgeHealthPort>"));
        assert!(server.contains(".is_healthy(&endpoint.health_url(), BRIDGE_SCHEMA_VERSION)"));
        assert!(server.contains("TcpListener::bind(BRIDGE_BIND_ADDRESS)"));
        assert!(server.contains("axum::serve(listener, router)"));
        assert!(!bridge_dto.contains("reqwest::"));
        assert!(!bridge_dto.contains("axum::"));
        assert!(!bridge_dto.contains("TcpListener"));
        assert!(actor.contains("bridge: Box<dyn AgentBridgePort>"));
        assert!(client.contains("bridge: Box<dyn AgentBridgePort>"));
        assert!(setup.contains("HttpBridgeHealth::new()"));
        assert!(setup.contains("HttpAgentBridge::new(tool_executor, bridge_health)"));
    }

    #[test]
    fn bridge_dispatches_exclusively_through_the_tool_registry() {
        let bridge = production_only(include_str!("adapters/http_bridge.rs"));
        assert!(bridge.contains("validate_tool_request(&tool_name, &body)"));
        assert!(bridge.contains("tool_timeout(&tool_name)"));
        assert!(bridge.contains("executor.execute(tool_name.clone(), body)"));
        assert!(!bridge.contains("collect_network_snapshot"));
        assert!(!bridge.contains("execute_network_probe"));
    }
}

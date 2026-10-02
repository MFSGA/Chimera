//! DNS override convergence at the fixed phases of the v2 transaction entry
//! points (`reconcile` / `stop` / `shutdown` / `recover_quarantine`).
//!
//! Legacy v1 entry points do not run the DNS converge tail. Protected runtime
//! launches still pass through the pre-activation gate below; the app process
//! continues to own any DNS behavior outside the injected controller's intent.
//!
//! Failure policy: converge-tail DNS failures are returned to the transaction
//! caller and keep their ownership record so later restore can still undo
//! them. A protected runtime cannot be launched until its pre-activation DNS
//! apply and read-back succeed; an apply error or timeout blocks that launch.
//! Read-back lives inside the controller; an `Err` never implies the side
//! effect is absent.
//!
//! One asymmetry, and it is the crash-recovery guarantee rather than a
//! preference: applying an override is refused when the record cannot be
//! written, because an override nothing recorded is an override nothing can
//! undo. Restoring is never blocked by a persistence failure — it is the safe
//! direction.

use std::{io, time::Duration};

use crate::{
    dns::{DnsController, DnsOverrideRecord, DnsOverrideState},
    runtime_store::RuntimeConfigStore,
};

use super::{CoreManager, Ctrl, EpochPlan};

const RECORD_FILE: &str = "dns-override.json";
const RECORD_STAGING_FILE: &str = "dns-override.json.tmp";

/// Startup orphan reconcile: a record left behind by a dead process is
/// restored (a no-op when its side effect never landed) and cleared. Runs
/// before the manager exists, so it takes the store directly.
///
/// Bounded by the same `dns_timeout` as the converge tail: this runs inside
/// manager construction, and a platform command that never returns would hang
/// the daemon before it ever serves.
pub(super) async fn reconcile_orphan_record(
    store: &RuntimeConfigStore,
    dns: Option<&dyn DnsController>,
    dns_timeout: Duration,
) {
    let path = store.dir().join(RECORD_FILE);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return;
    };
    let record = match serde_json::from_slice::<DnsOverrideRecord>(&bytes) {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!("unreadable orphan dns override record; keeping the file: {error}");
            return;
        }
    };
    let Some(dns) = dns else {
        tracing::warn!(
            "an orphan dns override record exists but no dns controller is injected; keeping it"
        );
        return;
    };
    match tokio::time::timeout(dns_timeout, dns.restore(&record)).await {
        Ok(Ok(())) => {
            if let Err(error) = tokio::fs::remove_file(&path).await {
                tracing::warn!("failed to clear the restored dns override record: {error}");
            }
        }
        Ok(Err(error)) => {
            tracing::warn!("orphan dns override restore failed; keeping the record: {error}");
        }
        Err(_) => tracing::warn!(
            "orphan dns override restore timed out after {dns_timeout:?}; keeping the record"
        ),
    }
}

impl CoreManager {
    /// Installs and verifies the host DNS override before launching a runtime
    /// whose effective config depends on UDP/TCP 53 interception. Keeping this
    /// ahead of process readiness prevents the system resolver from continuing
    /// to use its physical-service DNS while the new TUN is already active.
    /// On macOS, the manager also rejects such a plan when no host DNS
    /// controller was registered.
    ///
    /// Configurations without a host DNS intent are left alone here; the
    /// existing converge tail restores an owned override after the replacement
    /// runtime is healthy.
    pub(super) async fn dns_activate_for_plan(
        &self,
        ctrl: &mut Ctrl,
        plan: &EpochPlan,
    ) -> Result<(), crate::error::Error> {
        let Some(dns) = self.inner.dns.clone() else {
            #[cfg(target_os = "macos")]
            if crate::dns::macos::desired_dns_intent(&plan.effective_document).is_some() {
                return Err(crate::error::Error::ApplyFailed(
                    "this macOS config requires a host DNS controller, but none is registered"
                        .into(),
                ));
            }
            return Ok(());
        };
        let Some(intent) = dns.desired(&plan.effective_document) else {
            return Ok(());
        };

        let baseline = ctrl
            .dns_record
            .as_ref()
            .filter(|record| !record.interface.is_empty())
            .map(|record| (record.interface.clone(), record.previous.clone()));
        let pre_record = DnsOverrideRecord {
            interface: baseline
                .as_ref()
                .map_or_else(String::new, |(interface, _)| interface.clone()),
            previous: baseline
                .as_ref()
                .map_or_else(Vec::new, |(_, previous)| previous.clone()),
            applied: intent.servers.clone(),
            runtime_epoch: plan.revision.epoch.get(),
            owner_generation: None,
            state: DnsOverrideState::Applied,
        };
        self.persist_dns_record(ctrl, Some(pre_record))
            .await
            .map_err(|error| {
                crate::error::Error::ApplyFailed(format!(
                    "could not persist DNS override ownership before activation: {error}"
                ))
            })?;

        let dns_timeout = self.inner.options.dns_timeout;
        let mut record = match tokio::time::timeout(
            dns_timeout,
            dns.apply(&intent, plan.revision.epoch),
        )
        .await
        {
            Ok(Ok(record)) => record,
            Ok(Err(error)) => {
                return Err(crate::error::Error::ApplyFailed(format!(
                    "DNS override activation failed: {error}"
                )));
            }
            Err(_) => {
                return Err(crate::error::Error::ApplyFailed(format!(
                    "DNS override activation timed out after {dns_timeout:?}"
                )));
            }
        };

        if let Some((interface, previous)) = baseline {
            if record.interface != interface {
                return Err(crate::error::Error::ApplyFailed(
                    "DNS interface changed while the override was active".into(),
                ));
            }
            record.previous = previous;
        }
        self.persist_dns_record(ctrl, Some(record))
            .await
            .map_err(|error| {
                crate::error::Error::ApplyFailed(format!(
                    "could not persist DNS override read-back before activation: {error}"
                ))
            })
    }

    /// Before moving to a plan that no longer needs the host DNS override,
    /// restore and verify the previous resolver while the old protected plan
    /// is still available. If restoration is uncertain, the caller must not
    /// start or commit the unprotected target plan.
    pub(super) async fn dns_deactivate_for_plan(
        &self,
        ctrl: &mut Ctrl,
        plan: &EpochPlan,
    ) -> Result<(), crate::error::Error> {
        if ctrl.dns_record.is_none() {
            return Ok(());
        }
        let target_needs_override = self
            .inner
            .dns
            .as_ref()
            .and_then(|dns| dns.desired(&plan.effective_document))
            .is_some();
        if target_needs_override {
            return Ok(());
        }
        self.dns_restore(ctrl).await
    }

    /// Converge tail: apply the override when a runtime is up and wants one,
    /// restore otherwise. Idempotent; called with the control lock held.
    pub(super) async fn dns_converge(&self, ctrl: &mut Ctrl) -> Result<(), crate::error::Error> {
        let Some(dns) = self.inner.dns.clone() else {
            // No record means the host integration has nothing to reconcile.
            // A durable record without its controller is uncertain ownership,
            // and must be surfaced instead of reported as converged.
            return self.dns_restore(ctrl).await;
        };
        let desired = ctrl.current.as_ref().and_then(|active| {
            let running = !active.instance.state().borrow().state.is_terminal();
            running
                .then(|| {
                    dns.desired(&active.plan.effective_document)
                        .map(|intent| (intent, active.instance.epoch()))
                })
                .flatten()
        });
        let Some((intent, epoch)) = desired else {
            if let Err(error) = self.dns_restore(ctrl).await {
                tracing::warn!("dns override restore failed during convergence: {error}");
                return Err(error);
            }
            return Ok(());
        };

        // The baseline is captured once, on the converge that first took
        // ownership. Every later converge re-applies over an interface that
        // already carries *our* override, so the controller's read-back would
        // report the override itself as the thing to restore to.
        //
        // A non-empty `interface` is the discriminator, because only a
        // successful read-back supplies one -- the pre-record written before
        // the first apply deliberately leaves it empty. An empty `previous` is
        // a legitimate baseline (an interface with no resolvers), and
        // `RestorePending` still owns the baseline: it says the restore is
        // uncertain, not that what we recorded stopped being true.
        let baseline = ctrl
            .dns_record
            .as_ref()
            .filter(|record| !record.interface.is_empty())
            .map(|record| (record.interface.clone(), record.previous.clone()));

        // Persist a pre-record before the side effect: a crash between the
        // two leaves an orphan whose restore is a read-back no-op.
        let pre_record = DnsOverrideRecord {
            interface: baseline
                .as_ref()
                .map_or_else(String::new, |(interface, _)| interface.clone()),
            previous: baseline
                .as_ref()
                .map_or_else(Vec::new, |(_, previous)| previous.clone()),
            applied: intent.servers.clone(),
            runtime_epoch: epoch.get(),
            owner_generation: None,
            state: DnsOverrideState::Applied,
        };
        if let Err(error) = self.persist_dns_record(ctrl, Some(pre_record)).await {
            tracing::warn!("the dns override record is unwritable; skipping the override: {error}");
            return Err(crate::error::Error::ApplyFailed(format!(
                "could not persist DNS override ownership during convergence: {error}"
            )));
        }

        let dns_timeout = self.inner.options.dns_timeout;
        match tokio::time::timeout(dns_timeout, dns.apply(&intent, epoch)).await {
            Ok(Ok(mut record)) => {
                if let Some((interface, previous)) = baseline {
                    if record.interface != interface {
                        tracing::warn!(
                            "the dns interface changed under the override; keeping the recorded baseline"
                        );
                        return Err(crate::error::Error::ApplyFailed(
                            "DNS interface changed during convergence; ownership record retained"
                                .into(),
                        ));
                    }
                    record.previous = previous;
                }
                if let Err(error) = self.persist_dns_record(ctrl, Some(record)).await {
                    tracing::warn!(
                        "failed to persist the dns read-back; keeping the pre-record: {error}"
                    );
                    return Err(crate::error::Error::ApplyFailed(format!(
                        "could not persist DNS override read-back during convergence: {error}"
                    )));
                }
                Ok(())
            }
            // The side effect is uncertain either way, so the pre-record stays:
            // a later restore must still be able to undo it.
            Ok(Err(error)) => Err(crate::error::Error::ApplyFailed(format!(
                "DNS override convergence failed; ownership record retained: {error}"
            ))),
            Err(_) => Err(crate::error::Error::ApplyFailed(format!(
                "DNS override convergence timed out after {dns_timeout:?}; ownership record retained"
            ))),
        }
    }

    /// DNS is part of a successful runtime transaction, so public manager
    /// entry points must not return success while the host resolver state is
    /// uncertain. Preserve the original error when no DNS side effect is
    /// needed; when both sides fail, report both facts.
    pub(super) async fn finish_dns_converge<T>(
        &self,
        ctrl: &mut Ctrl,
        result: Result<T, crate::error::Error>,
    ) -> Result<T, crate::error::Error> {
        let runtime_alive = ctrl
            .current
            .as_ref()
            .is_some_and(|active| !active.instance.state().borrow().state.is_terminal());
        let dns_result = if result.is_ok() || !runtime_alive {
            self.dns_converge(ctrl).await
        } else {
            Ok(())
        };
        match (result, dns_result) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Ok(_), Err(dns_error)) => Err(crate::error::Error::ApplyFailed(format!(
                "runtime transition completed, but host DNS state is unconfirmed: {dns_error}"
            ))),
            (Err(operation_error), Ok(())) => Err(operation_error),
            (Err(operation_error), Err(dns_error)) => {
                Err(crate::error::Error::ApplyFailed(format!(
                    "runtime transition failed ({operation_error}); host DNS convergence also failed: {dns_error}"
                )))
            }
        }
    }

    /// Restore head/tail: undo the recorded override and clear the record.
    /// Called at the head of stop/shutdown (so resolution never points at a
    /// core being torn down) and from the converge tail when nothing runs.
    pub(super) async fn dns_restore(&self, ctrl: &mut Ctrl) -> Result<(), crate::error::Error> {
        let Some(record) = ctrl.dns_record.clone() else {
            return Ok(());
        };
        let Some(dns) = self.inner.dns.clone() else {
            return Err(crate::error::Error::ApplyFailed(
                "DNS override is recorded but no host DNS controller is registered".into(),
            ));
        };
        let mut pending = record.clone();
        pending.state = DnsOverrideState::RestorePending;
        if let Err(error) = self.persist_dns_record(ctrl, Some(pending)).await {
            // Undoing an override is the safe direction; a record that cannot
            // be updated must not keep the system pointed at a dead core.
            tracing::warn!("failed to mark the dns override restore-pending: {error}");
        }
        let dns_timeout = self.inner.options.dns_timeout;
        match tokio::time::timeout(dns_timeout, dns.restore(&record)).await {
            Ok(Ok(())) => {
                if let Err(error) = self.persist_dns_record(ctrl, None).await {
                    tracing::warn!("dns was restored but its record could not be cleared: {error}");
                }
                Ok(())
            }
            Ok(Err(error)) => Err(crate::error::Error::ApplyFailed(format!(
                "DNS override restore failed; ownership record retained: {error}"
            ))),
            Err(_) => Err(crate::error::Error::ApplyFailed(format!(
                "DNS override restore timed out after {dns_timeout:?}; ownership record retained"
            ))),
        }
    }

    /// Record-before-side-effect persistence, durably: a torn write would
    /// leave an orphan record that cannot be parsed and therefore cannot be
    /// undone, and an unsynced one could vanish in the crash it exists to
    /// survive. Same protocol as [`RuntimeConfigStore`](crate::runtime_store):
    /// write, flush and `sync_all` a staging file, publish it atomically, then
    /// sync the directory entry.
    ///
    /// In-memory state advances once the record is *visible* -- after the
    /// rename, not after the directory sync. A failed directory sync leaves a
    /// readable record behind, so pretending we do not own it would be the
    /// bigger lie; it is reported and the ownership stands.
    async fn persist_dns_record(
        &self,
        ctrl: &mut Ctrl,
        record: Option<DnsOverrideRecord>,
    ) -> io::Result<()> {
        use chimera_utils::io::atomic_fs;
        use tokio::io::AsyncWriteExt;

        let dir = self.inner.store.dir();
        let path = dir.join(RECORD_FILE);
        match &record {
            Some(entry) => {
                let staging = dir.join(RECORD_STAGING_FILE);
                let result = async {
                    let bytes = serde_json::to_vec_pretty(entry).map_err(io::Error::other)?;
                    let mut file = tokio::fs::File::create(&staging).await?;
                    file.write_all(&bytes).await?;
                    file.flush().await?;
                    file.sync_all().await?;
                    drop(file);
                    // Same split as the runtime config store: `atomic_replace`
                    // requires an existing target on Windows, `atomic_move_new`
                    // requires an absent one. The runtime directory is held
                    // under an ownership lock, so nothing else publishes here.
                    // A `try_exists` failure is propagated rather than read as
                    // "absent": we cannot publish safely into a directory we
                    // cannot stat.
                    if tokio::fs::try_exists(&path).await? {
                        atomic_fs::atomic_replace(atomic_fs::AtomicReplacement {
                            replacement: staging.as_std_path(),
                            destination: path.as_std_path(),
                        })
                        .await
                    } else {
                        atomic_fs::atomic_move_new(&staging, &path).await
                    }
                    .map_err(io::Error::other)
                }
                .await;
                if let Err(error) = result {
                    let _ = tokio::fs::remove_file(&staging).await;
                    return Err(error);
                }
                if let Err(error) = atomic_fs::sync_dir(dir).await {
                    tracing::warn!(
                        "the dns override record is published but its directory entry is unsynced: {error}"
                    );
                }
            }
            None => match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    if let Err(error) = atomic_fs::sync_dir(dir).await {
                        tracing::warn!(
                            "the dns override record is removed but its directory entry is unsynced: {error}"
                        );
                    }
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                Err(_) => {}
            },
        }
        ctrl.dns_record = record;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{
        Epoch,
        capability::{Feature, RuntimeFeature},
        kind::CoreKind,
        runtime::{BoxFuture, RuntimeBackend, RuntimeInstance, RuntimeLaunchRequest},
        spec::{CoreSpec, InstanceOptions, InstanceSpec, ManagerOptions, ResolvedController},
        state::ConfigRevision,
    };
    use camino::Utf8Path;
    use enumset::EnumSet;

    struct FailingDnsController {
        applies: AtomicUsize,
        restores: AtomicUsize,
        wants_dns: bool,
        restore_fails: bool,
    }

    impl DnsController for FailingDnsController {
        fn desired(&self, _effective: &serde_yaml_ng::Mapping) -> Option<crate::DnsIntent> {
            self.wants_dns.then(|| crate::DnsIntent {
                servers: vec!["192.0.2.1".into()],
            })
        }

        fn apply<'a>(
            &'a self,
            _intent: &'a crate::DnsIntent,
            _runtime_epoch: Epoch,
        ) -> BoxFuture<'a, Result<DnsOverrideRecord, crate::DnsError>> {
            self.applies.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Err(crate::DnsError::Command(
                    "injected activation failure".into(),
                ))
            })
        }

        fn restore<'a>(
            &'a self,
            _record: &'a DnsOverrideRecord,
        ) -> BoxFuture<'a, Result<(), crate::DnsError>> {
            self.restores.fetch_add(1, Ordering::SeqCst);
            let restore_fails = self.restore_fails;
            Box::pin(async move {
                if restore_fails {
                    Err(crate::DnsError::Command("injected restore failure".into()))
                } else {
                    Ok(())
                }
            })
        }
    }

    struct CountingBackend(AtomicUsize);

    impl RuntimeBackend for CountingBackend {
        fn launch(
            &self,
            _request: RuntimeLaunchRequest,
        ) -> BoxFuture<'_, Result<Box<dyn RuntimeInstance>, crate::Error>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(crate::Error::NotStarted) })
        }

        fn check_config<'a>(
            &'a self,
            _spec: &'a InstanceSpec,
        ) -> BoxFuture<'a, Result<(), crate::Error>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn test_plan(root: &Utf8Path, effective_document: serde_yaml_ng::Mapping) -> EpochPlan {
        let epoch = Epoch::new(1).unwrap();
        let spec = InstanceSpec {
            core: CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: root.join("mihomo"),
                version: Some("test".into()),
                features: vec![],
            },
            config_path: root.join("source.yaml"),
            working_dir: root.to_owned(),
            pid_file: None,
            options: InstanceOptions::default(),
        };
        EpochPlan {
            source_spec: spec.clone(),
            effective_spec: spec,
            controller: ResolvedController {
                host: clash_api::Host::http("127.0.0.1:9090").unwrap(),
                secret: None,
            },
            revision: ConfigRevision {
                epoch,
                generation: 1,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
                runtime_path: root.join("runtime/config-1.yaml"),
            },
            capabilities: EnumSet::<Feature>::empty(),
            runtime_features: EnumSet::<RuntimeFeature>::empty(),
            source_document: serde_yaml_ng::Mapping::new(),
            effective_document,
        }
    }

    fn protected_config() -> serde_yaml_ng::Mapping {
        serde_yaml_ng::from_str(
            "dns:\n  enable: true\n  enhanced-mode: fake-ip\ntun:\n  enable: true\n  route-all: true\n  dns-hijack:\n    - any:53\n    - tcp://any:53\n",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn dns_activation_failure_blocks_runtime_launch() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let backend = Arc::new(CountingBackend(AtomicUsize::new(0)));
        let dns = Arc::new(FailingDnsController {
            applies: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
            wants_dns: true,
            restore_fails: false,
        });
        let options = ManagerOptions {
            runtime_dir: Some(root.join("runtime")),
            log_sink_enabled: false,
            ..ManagerOptions::default()
        };
        let manager = CoreManager::builder(options)
            .runtime_backend(backend.clone())
            .dns_controller(dns.clone())
            .build()
            .await
            .unwrap();

        let plan = test_plan(&root, serde_yaml_ng::Mapping::new());

        let mut ctrl = manager.inner.ctrl.lock().await;
        let error = manager
            .start_prepared(&mut ctrl, plan)
            .await
            .expect_err("DNS override activation must gate runtime launch");

        assert!(matches!(error, crate::Error::ApplyFailed(_)));
        assert_eq!(dns.applies.load(Ordering::SeqCst), 1);
        assert_eq!(dns.restores.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dns_restore_failure_blocks_unprotected_runtime_launch() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let backend = Arc::new(CountingBackend(AtomicUsize::new(0)));
        let dns = Arc::new(FailingDnsController {
            applies: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
            wants_dns: false,
            restore_fails: true,
        });
        let options = ManagerOptions {
            runtime_dir: Some(root.join("runtime")),
            log_sink_enabled: false,
            ..ManagerOptions::default()
        };
        let manager = CoreManager::builder(options)
            .runtime_backend(backend.clone())
            .dns_controller(dns.clone())
            .build()
            .await
            .unwrap();
        let record = DnsOverrideRecord {
            interface: "test-dns-key".into(),
            previous: vec!["192.0.2.53".into()],
            applied: vec!["192.0.2.1".into()],
            runtime_epoch: 1,
            owner_generation: None,
            state: DnsOverrideState::Applied,
        };

        let mut ctrl = manager.inner.ctrl.lock().await;
        manager
            .persist_dns_record(&mut ctrl, Some(record))
            .await
            .unwrap();
        let plan = test_plan(&root, serde_yaml_ng::Mapping::new());

        let error = manager
            .start_prepared(&mut ctrl, plan)
            .await
            .expect_err("an unprotected runtime must not launch before DNS is restored");

        assert!(matches!(error, crate::Error::ApplyFailed(_)));
        assert_eq!(dns.restores.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
        assert!(ctrl.dns_record.is_some());
    }

    #[tokio::test]
    async fn dns_restore_failure_aborts_stop_and_keeps_ownership_record() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let dns = Arc::new(FailingDnsController {
            applies: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
            wants_dns: true,
            restore_fails: true,
        });
        let options = ManagerOptions {
            runtime_dir: Some(root.join("runtime")),
            log_sink_enabled: false,
            ..ManagerOptions::default()
        };
        let manager = CoreManager::builder(options)
            .dns_controller(dns.clone())
            .build()
            .await
            .unwrap();
        let record = DnsOverrideRecord {
            interface: "test-dns-key".into(),
            previous: vec!["192.0.2.53".into()],
            applied: vec!["192.0.2.1".into()],
            runtime_epoch: 1,
            owner_generation: None,
            state: DnsOverrideState::Applied,
        };

        let mut ctrl = manager.inner.ctrl.lock().await;
        manager
            .persist_dns_record(&mut ctrl, Some(record))
            .await
            .unwrap();
        drop(ctrl);

        let error = manager
            .stop()
            .await
            .expect_err("stop must not tear down the TUN if DNS restore is uncertain");

        assert!(matches!(error, crate::Error::ApplyFailed(_)));
        assert_eq!(dns.restores.load(Ordering::SeqCst), 1);
        assert!(manager.inner.ctrl.lock().await.dns_record.is_some());
    }

    #[tokio::test]
    async fn dns_restore_failure_is_reported_by_convergence() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let dns = Arc::new(FailingDnsController {
            applies: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
            wants_dns: true,
            restore_fails: true,
        });
        let options = ManagerOptions {
            runtime_dir: Some(root.join("runtime")),
            log_sink_enabled: false,
            ..ManagerOptions::default()
        };
        let manager = CoreManager::builder(options)
            .dns_controller(dns.clone())
            .build()
            .await
            .unwrap();
        let record = DnsOverrideRecord {
            interface: "test-dns-key".into(),
            previous: vec!["192.0.2.53".into()],
            applied: vec!["192.0.2.1".into()],
            runtime_epoch: 1,
            owner_generation: None,
            state: DnsOverrideState::Applied,
        };

        let mut ctrl = manager.inner.ctrl.lock().await;
        manager
            .persist_dns_record(&mut ctrl, Some(record))
            .await
            .unwrap();

        let error = manager
            .finish_dns_converge(&mut ctrl, Ok(()))
            .await
            .expect_err("manager must not report success with an uncertain host DNS restore");

        assert!(matches!(error, crate::Error::ApplyFailed(_)));
        assert_eq!(dns.restores.load(Ordering::SeqCst), 1);
        assert!(ctrl.dns_record.is_some());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn missing_macos_dns_controller_blocks_protected_launch() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let backend = Arc::new(CountingBackend(AtomicUsize::new(0)));
        let options = ManagerOptions {
            runtime_dir: Some(root.join("runtime")),
            log_sink_enabled: false,
            ..ManagerOptions::default()
        };
        let manager = CoreManager::builder(options)
            .runtime_backend(backend.clone())
            .build()
            .await
            .unwrap();
        let plan = test_plan(&root, protected_config());

        let mut ctrl = manager.inner.ctrl.lock().await;
        let error = manager
            .start_prepared(&mut ctrl, plan)
            .await
            .expect_err("a protected macOS config must not launch without DNS control");

        assert!(matches!(error, crate::Error::ApplyFailed(_)));
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    }
}

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::{Result, anyhow};
use serde_yaml::{Mapping, Value};
use tokio::{
    sync::Barrier,
    time::{Duration, sleep},
};

use super::*;

fn initial_config() -> ClashConfig {
    ClashConfig {
        port: None,
        mode: Some("rule".to_string()),
        ipv6: Some(true),
        socket_port: None,
        allow_lan: Some(false),
        log_level: Some("info".to_string()),
        mixed_port: Some(7890),
        redir_port: None,
        socks_port: None,
        tproxy_port: None,
        external_controller: Some("127.0.0.1:9090".to_string()),
        secret: None,
    }
}

fn allow_lan_patch(enabled: bool) -> Mapping {
    Mapping::from_iter([("allow-lan".into(), Value::Bool(enabled))])
}

fn mode_patch(mode: &str) -> Mapping {
    Mapping::from_iter([("mode".into(), Value::String(mode.to_string()))])
}

fn update_config(state: &Arc<Mutex<ClashConfig>>, patch: Mapping) -> Result<()> {
    let mut config = state.lock().expect("config mutex should not be poisoned");
    let mut value = serde_yaml::to_value(&*config)?;
    let mapping = value
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("config must serialize to a mapping"))?;
    mapping.extend(patch);
    *config = serde_yaml::from_value(value)?;
    Ok(())
}

fn snapshot(state: &Arc<Mutex<ClashConfig>>) -> ClashConfig {
    state.lock().expect("config mutex").clone()
}

#[tokio::test]
async fn empty_patch_is_a_noop() {
    let coordinator = RuntimePatchCoordinator::default();
    let reads = Arc::new(AtomicUsize::new(0));
    let patches = Arc::new(AtomicUsize::new(0));
    let persists = Arc::new(AtomicUsize::new(0));

    coordinator
        .apply(
            Mapping::new(),
            {
                let reads = Arc::clone(&reads);
                move || {
                    reads.fetch_add(1, Ordering::SeqCst);
                    async { Ok(initial_config()) }
                }
            },
            {
                let patches = Arc::clone(&patches);
                move |_patch| {
                    patches.fetch_add(1, Ordering::SeqCst);
                    async { Ok(()) }
                }
            },
            {
                let persists = Arc::clone(&persists);
                move |_patch| {
                    persists.fetch_add(1, Ordering::SeqCst);
                    async { Ok(()) }
                }
            },
        )
        .await
        .expect("empty patch should succeed");

    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(patches.load(Ordering::SeqCst), 0);
    assert_eq!(persists.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn persists_after_core_verification() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let persisted = Arc::new(Mutex::new(Vec::new()));

    coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                move |patch| {
                    let state = Arc::clone(&state);
                    async move { update_config(&state, patch) }
                }
            },
            {
                let persisted = Arc::clone(&persisted);
                move |patch| {
                    let persisted = Arc::clone(&persisted);
                    async move {
                        persisted.lock().expect("persist mutex").push(patch);
                        Ok(())
                    }
                }
            },
        )
        .await
        .expect("verified patch should persist");

    assert_eq!(snapshot(&state).allow_lan, Some(true));
    assert_eq!(
        persisted.lock().expect("persist mutex").as_slice(),
        &[allow_lan_patch(true)]
    );
}

#[tokio::test]
async fn compensates_a_partially_applied_patch_error() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let calls = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                let calls = Arc::clone(&calls);
                move |patch| {
                    let state = Arc::clone(&state);
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        update_config(&state, patch)?;
                        if call == 0 {
                            Err(anyhow!("patch endpoint failed after partial apply"))
                        } else {
                            Ok(())
                        }
                    }
                }
            },
            |_patch| async { panic!("persistence must not run") },
        )
        .await
        .expect_err("partial patch failure should be reported");

    assert!(error.to_string().contains("partial apply"));
    assert_eq!(snapshot(&state).allow_lan, Some(false));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn compensates_when_read_back_fails() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let reads = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                let reads = Arc::clone(&reads);
                move || {
                    let state = Arc::clone(&state);
                    let call = reads.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call == 1 {
                            Err(anyhow!("read-back unavailable"))
                        } else {
                            Ok(snapshot(&state))
                        }
                    }
                }
            },
            {
                let state = Arc::clone(&state);
                move |patch| {
                    let state = Arc::clone(&state);
                    async move { update_config(&state, patch) }
                }
            },
            |_patch| async { panic!("persistence must not run") },
        )
        .await
        .expect_err("read-back failure should be reported");

    assert!(error.to_string().contains("read-back unavailable"));
    assert_eq!(snapshot(&state).allow_lan, Some(false));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn compensates_when_core_ignores_the_requested_patch() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let patches = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                let patches = Arc::clone(&patches);
                move |patch| {
                    let state = Arc::clone(&state);
                    let call = patches.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call > 0 {
                            update_config(&state, patch)?;
                        }
                        Ok(())
                    }
                }
            },
            |_patch| async { panic!("persistence must not run") },
        )
        .await
        .expect_err("ignored patch should fail verification");

    assert!(error.to_string().contains("did not apply"));
    assert_eq!(snapshot(&state).allow_lan, Some(false));
    assert_eq!(patches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn persistence_failure_rolls_back_only_requested_fields() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                move |patch| {
                    let state = Arc::clone(&state);
                    async move { update_config(&state, patch) }
                }
            },
            {
                let state = Arc::clone(&state);
                move |_patch| {
                    let state = Arc::clone(&state);
                    async move {
                        update_config(&state, mode_patch("global"))?;
                        Err(anyhow!("persistence failed"))
                    }
                }
            },
        )
        .await
        .expect_err("persistence failure should be reported");

    let restored = snapshot(&state);
    assert!(error.to_string().contains("persistence failed"));
    assert_eq!(restored.allow_lan, Some(false));
    assert_eq!(restored.mode.as_deref(), Some("global"));
}

#[tokio::test]
async fn reports_primary_and_rollback_patch_failures() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let calls = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                let calls = Arc::clone(&calls);
                move |patch| {
                    let state = Arc::clone(&state);
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call == 0 {
                            update_config(&state, patch)
                        } else {
                            Err(anyhow!("rollback endpoint unavailable"))
                        }
                    }
                }
            },
            |_patch| async { Err(anyhow!("desired state save failed")) },
        )
        .await
        .expect_err("rollback failure should be reported");

    let message = error.to_string();
    assert!(message.contains("desired state save failed"));
    assert!(message.contains("rollback endpoint unavailable"));
    assert_eq!(snapshot(&state).allow_lan, Some(true));
}

#[tokio::test]
async fn reports_when_rollback_read_back_fails() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let reads = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                let reads = Arc::clone(&reads);
                move || {
                    let state = Arc::clone(&state);
                    let call = reads.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call == 2 {
                            Err(anyhow!("rollback read failed"))
                        } else {
                            Ok(snapshot(&state))
                        }
                    }
                }
            },
            {
                let state = Arc::clone(&state);
                move |patch| {
                    let state = Arc::clone(&state);
                    async move { update_config(&state, patch) }
                }
            },
            |_patch| async { Err(anyhow!("persist failed")) },
        )
        .await
        .expect_err("rollback read failure should be reported");

    let message = error.to_string();
    assert!(message.contains("persist failed"));
    assert!(message.contains("rollback read failed"));
    assert_eq!(snapshot(&state).allow_lan, Some(false));
}

#[tokio::test]
async fn reports_when_rollback_does_not_restore_the_snapshot() {
    let coordinator = RuntimePatchCoordinator::default();
    let state = Arc::new(Mutex::new(initial_config()));
    let calls = Arc::new(AtomicUsize::new(0));

    let error = coordinator
        .apply(
            allow_lan_patch(true),
            {
                let state = Arc::clone(&state);
                move || {
                    let state = Arc::clone(&state);
                    async move { Ok(snapshot(&state)) }
                }
            },
            {
                let state = Arc::clone(&state);
                let calls = Arc::clone(&calls);
                move |patch| {
                    let state = Arc::clone(&state);
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call == 0 {
                            update_config(&state, patch)?;
                        }
                        Ok(())
                    }
                }
            },
            |_patch| async { Err(anyhow!("persist failed")) },
        )
        .await
        .expect_err("unrestored rollback should be reported");

    assert!(error.to_string().contains("rollback verification failed"));
    assert_eq!(snapshot(&state).allow_lan, Some(true));
}

#[tokio::test]
async fn coordinator_serializes_concurrent_transactions() {
    let coordinator = Arc::new(RuntimePatchCoordinator::default());
    let state = Arc::new(Mutex::new(initial_config()));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(3));

    let spawn_transaction = |requested: Mapping| {
        let coordinator = Arc::clone(&coordinator);
        let state = Arc::clone(&state);
        let active = Arc::clone(&active);
        let max_active = Arc::clone(&max_active);
        let start = Arc::clone(&start);

        tokio::spawn(async move {
            start.wait().await;
            coordinator
                .apply(
                    requested,
                    {
                        let state = Arc::clone(&state);
                        move || {
                            let state = Arc::clone(&state);
                            async move { Ok(snapshot(&state)) }
                        }
                    },
                    {
                        let state = Arc::clone(&state);
                        let active = Arc::clone(&active);
                        let max_active = Arc::clone(&max_active);
                        move |patch| {
                            let state = Arc::clone(&state);
                            let active = Arc::clone(&active);
                            let max_active = Arc::clone(&max_active);
                            async move {
                                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                                max_active.fetch_max(current, Ordering::SeqCst);
                                sleep(Duration::from_millis(30)).await;
                                let result = update_config(&state, patch);
                                active.fetch_sub(1, Ordering::SeqCst);
                                result
                            }
                        }
                    },
                    |_patch| async {
                        sleep(Duration::from_millis(10)).await;
                        Ok(())
                    },
                )
                .await
        })
    };

    let first = spawn_transaction(allow_lan_patch(true));
    let second = spawn_transaction(allow_lan_patch(false));
    start.wait().await;

    first
        .await
        .expect("first task should join")
        .expect("first transaction should commit");
    second
        .await
        .expect("second task should join")
        .expect("second transaction should commit");

    assert_eq!(max_active.load(Ordering::SeqCst), 1);
}

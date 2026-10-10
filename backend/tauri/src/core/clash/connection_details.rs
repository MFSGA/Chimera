//! Ref-aligned, window-owned delivery adapter for on-demand connection detail frames.
//! The Clash connector owns the latest watch frame; this adapter only tracks
//! native Channel subscriptions, with no additional connection state source.
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tauri::ipc::Channel;
use tokio::{sync::watch, task::JoinHandle};

use super::ws::ClashConnectionDetails;

pub type SubscriptionId = u64;

struct Subscription {
    webview: String,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub struct ConnectionDetailSubscriptions {
    next_id: AtomicU64,
    subscriptions: Mutex<HashMap<SubscriptionId, Subscription>>,
}

impl ConnectionDetailSubscriptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, webview: String, task: JoinHandle<()>) -> SubscriptionId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.subscriptions
            .lock()
            .expect("connection detail subscriptions poisoned")
            .insert(id, Subscription { webview, task });
        id
    }

    /// No other webview may cancel a subscription, even if it knows its id.
    pub fn unsubscribe(&self, id: SubscriptionId, webview: &str) -> anyhow::Result<()> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .expect("connection detail subscriptions poisoned");
        if let Some(subscription) = subscriptions.get(&id) {
            anyhow::ensure!(
                subscription.webview == webview,
                "connection detail subscription belongs to another webview"
            );
        }
        if let Some(subscription) = subscriptions.remove(&id) {
            subscription.task.abort();
        }
        Ok(())
    }

    pub fn cancel_for_webview(&self, webview: &str) {
        self.subscriptions
            .lock()
            .expect("connection detail subscriptions poisoned")
            .retain(|_, subscription| {
                if subscription.webview == webview {
                    subscription.task.abort();
                    false
                } else {
                    true
                }
            });
    }
}

/// Sends the most recent frame first, then each subsequent sample. The receiver
/// is dropped on unsubscription, webview close, or backend shutdown.
pub async fn forward_details(
    mut receiver: watch::Receiver<Option<std::sync::Arc<ClashConnectionDetails>>>,
    channel: Channel<ClashConnectionDetails>,
) {
    if let Some(frame) = receiver.borrow_and_update().clone()
        && channel.send((*frame).clone()).is_err()
    {
        return;
    }
    while receiver.changed().await.is_ok() {
        if let Some(frame) = receiver.borrow_and_update().clone()
            && channel.send((*frame).clone()).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ownership_and_cancellation_drop_the_detail_receiver() {
        let (sender, receiver) =
            watch::channel(Some(std::sync::Arc::new(ClashConnectionDetails {
                sequence: 1,
                connections: vec![],
            })));
        let registry = ConnectionDetailSubscriptions::new();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = ready_tx.send(());
            let mut receiver = receiver;
            let _ = receiver.changed().await;
        });
        let id = registry.register("main".to_string(), task);
        ready_rx.await.expect("receiver spawned");
        assert_eq!(sender.receiver_count(), 1);
        assert!(registry.unsubscribe(id, "legacy").is_err());
        assert_eq!(sender.receiver_count(), 1);
        registry.unsubscribe(id, "main").unwrap();
        tokio::task::yield_now().await;
        assert_eq!(sender.receiver_count(), 0);
    }

    #[tokio::test]
    async fn closing_a_window_cancels_only_its_subscriptions() {
        let registry = ConnectionDetailSubscriptions::new();
        let main = tokio::spawn(std::future::pending::<()>());
        let legacy = tokio::spawn(std::future::pending::<()>());
        let main_abort = main.abort_handle();
        let legacy_abort = legacy.abort_handle();
        registry.register("main".to_string(), main);
        registry.register("legacy".to_string(), legacy);
        registry.cancel_for_webview("main");
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !main_abort.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("main forwarding task stops after window closes");
        assert!(!legacy_abort.is_finished());
        registry.cancel_for_webview("legacy");
    }
}

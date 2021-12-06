use log::{error, info, trace, warn};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::watch;
use tokio::time::sleep;

#[derive(Clone)]
pub struct ShutdownReceiver {
    shutdown_rx: watch::Receiver<bool>,
    shutdown_confirm_tx: broadcast::Sender<bool>,
}

impl ShutdownReceiver {
    pub async fn watch(&mut self) {
        if let Err(e) = self.shutdown_rx.changed().await {
            error!(target:"rustydht_lib::ShutdownReceiver", "Error watching shutdown_rx. Sender has dropped? Err:{:?}", e);
        }
    }

    pub fn spawn_with_shutdown<T>(
        mut shutdown: ShutdownReceiver,
        todo: T,
        task_name: impl std::fmt::Display + Send + 'static + Sync,
        timeout: Option<Duration>,
    ) where
        T: std::future::Future + Send + 'static,
        T::Output: Send + 'static,
    {
        tokio::spawn(async move {
            trace!(target: "rustydht_lib::ShutdownReceiver", "Task '{}' starting up", task_name);
            tokio::select! {
                _ = shutdown.watch() => {}
                _ = todo => {}
                _ = async {
                    match timeout {
                        Some(timeout) => {
                            sleep(timeout).await;
                            trace!(target: "rustydht_lib::ShutdownReceiver", "Task '{}' timed out", task_name);
                        }
                        None => {std::future::pending::<bool>().await;}
                    };
                } => {}
            }
            trace!(target: "rustydht_lib::ShutdownReceiver", "Task '{}' terminating", task_name);
        });
    }
}

pub struct ShutdownSender {
    shutdown_tx: watch::Sender<bool>,
    shutdown_confirm_rx: broadcast::Receiver<bool>,
}

impl ShutdownSender {
    pub async fn shutdown(&mut self) {
        info!(target: "rustydht_lib::ShutdownSender", "Sending shutdown signal to tasks");
        if let Err(e) = self.shutdown_tx.send(true) {
            warn!(target: "rustydht_lib::ShutdownSender","Failed to send shutdown signal - likely all tasks are already stopped. Error: {:?}", e);
        }
        if let Err(_) = self.shutdown_confirm_rx.recv().await {
            // This error is expected
        }
        info!(target: "rustydht_lib::ShutdownSender","All tasks have stopped");
    }
}

pub fn create_shutdown() -> (ShutdownSender, ShutdownReceiver) {
    // We use this channel to send a shutdown notification to everybody
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // We use this channel to determine when all tasks have shutdown
    let (shutdown_confirm_tx, shutdown_confirm_rx) = broadcast::channel::<bool>(1);

    (
        ShutdownSender {
            shutdown_tx: shutdown_tx,
            shutdown_confirm_rx: shutdown_confirm_rx,
        },
        ShutdownReceiver {
            shutdown_rx: shutdown_rx,
            shutdown_confirm_tx: shutdown_confirm_tx,
        },
    )
}

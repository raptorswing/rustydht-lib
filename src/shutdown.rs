use log::{info, warn};
use tokio::sync::broadcast;
use tokio::sync::watch;

#[derive(Clone)]
pub struct ShutdownReceiver {
    shutdown_rx: watch::Receiver<bool>,
    shutdown_confirm_tx: broadcast::Sender<bool>,
}

impl ShutdownReceiver {
    pub async fn watch(&mut self) {
        self.shutdown_rx.changed().await;
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

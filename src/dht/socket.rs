use crate::common::Id;
use crate::errors::RustyDHTError;
use crate::packets;
use crate::shutdown::ShutdownReceiver;
use crate::storage::outbound_request_storage::{OutboundRequestStorage, RequestInfo};
use anyhow::anyhow;
use log::{error, trace, warn};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::interval;

type MessagePair = (packets::Message, SocketAddr);

pub struct DHTSocket {
    recv_from_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<MessagePair>>>,
    send_to_tx: mpsc::Sender<MessagePair>,
    request_storage: Arc<Mutex<OutboundRequestStorage>>,
}

impl DHTSocket {
    pub fn new(shutdown: ShutdownReceiver, socket: UdpSocket) -> DHTSocket {
        let (send_to_tx, send_to_rx) = mpsc::channel(128);
        let (recv_from_tx, recv_from_rx) = mpsc::channel(128);
        let request_storage = Arc::new(Mutex::new(OutboundRequestStorage::new()));
        let socket = Arc::new(socket);
        ShutdownReceiver::spawn_with_shutdown(
            shutdown.clone(),
            DHTSocket::background_io_outgoing(socket.clone(), send_to_rx),
            "DHTSocket background outgoing I/O task",
            None,
        );
        ShutdownReceiver::spawn_with_shutdown(
            shutdown.clone(),
            DHTSocket::background_io_incoming(socket, recv_from_tx, request_storage.clone()),
            "DHTSocket background incoming I/O task",
            None,
        );
        ShutdownReceiver::spawn_with_shutdown(
            shutdown,
            DHTSocket::request_cleanup(request_storage.clone()),
            "DHTSocket background request cleanup task",
            None,
        );
        DHTSocket {
            recv_from_rx: Arc::new(tokio::sync::Mutex::new(recv_from_rx)),
            send_to_tx: send_to_tx,
            request_storage: request_storage,
        }
    }

    pub async fn recv_from(&self) -> Result<MessagePair, RustyDHTError> {
        match self.recv_from_rx.lock().await.recv().await {
            Some(message_pair) => Ok(message_pair),
            None => Err(RustyDHTError::GeneralError(anyhow!(
                "Can't recv_from as background I/O task channel has closed"
            ))),
        }
    }

    pub async fn send_to(
        &self,
        to_send: packets::Message,
        dest: SocketAddr,
        dest_id: Option<Id>,
    ) -> Result<Option<mpsc::Receiver<packets::Message>>, RustyDHTError> {
        let mut to_ret = None;
        // optimization to only store notification stuff on requests (not on replies too)
        if let packets::MessageType::Request(_) = to_send.message_type {
            let (notify_tx, notify_rx) = mpsc::channel(1);
            to_ret = Some(notify_rx);
            self.request_storage
                .lock()
                .unwrap()
                .add_request(RequestInfo::new(
                    dest,
                    dest_id,
                    to_send.clone(),
                    Some(notify_tx),
                ));
        }

        self.send_to_tx
            .send((to_send, dest))
            .await
            .map_err(|e| RustyDHTError::GeneralError(e.into()))?;
        Ok(to_ret)
    }

    async fn background_io_outgoing(
        socket: Arc<UdpSocket>,
        mut send_to_rx: mpsc::Receiver<MessagePair>,
    ) {
        loop {
            match DHTSocket::background_io_outgoing_single(&socket, &mut send_to_rx).await {
                Ok(_) => { /* Keep on truckin'!*/ }
                Err(e) => match e {
                    RustyDHTError::ConntrackError(_) => {
                        warn!(target: "rustydht_lib::DHTSocket", "Outgoing traffic may be dropped due to conntrack error: {:?}", e);
                    }
                    _ => {
                        error!(target: "rustydht_lib::DHTSocket", "Error in background outgoing I/O task:{:?}", e);
                        break;
                    }
                },
            }
        }
    }

    async fn background_io_outgoing_single(
        socket: &UdpSocket,
        send_to_rx: &mut mpsc::Receiver<MessagePair>,
    ) -> Result<(), RustyDHTError> {
        match send_to_rx.recv().await {
            None => Err(RustyDHTError::GeneralError(anyhow!(
                "send_to_rx channel is empty and closed"
            ))),
            Some((msg, dest)) => {
                let bytes = msg.to_bytes()?;
                trace!(target:"rustydht_lib::DHTSocket", "Sending {} bytes to {}", bytes.len(), dest);
                match socket.send_to(&bytes, dest).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        #[cfg(target_os = "linux")]
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            return Err(RustyDHTError::ConntrackError(anyhow!(
                                "send_to resulted in PermissionDenied. Is conntrack table full?"
                            )));
                        }
                        Err(RustyDHTError::GeneralError(e.into()))
                    }
                }
            }
        }
    }

    async fn background_io_incoming(
        socket: Arc<UdpSocket>,
        recv_from_tx: mpsc::Sender<MessagePair>,
        request_storage: Arc<Mutex<OutboundRequestStorage>>,
    ) {
        loop {
            match DHTSocket::background_io_incoming_single(&socket, &recv_from_tx, &request_storage)
                .await
            {
                Ok(_) => { /* Keep on truckin'!*/ }
                Err(e) => match e {
                    RustyDHTError::PacketParseError(_) => {
                        warn!(target: "rustydht_lib::DHTSocket", "Failed to parse incoming packet: {:?}", e);
                        continue;
                    }
                    _ => {
                        error!(target: "rustydht_lib::DHTSocket", "Error in background incoming I/O task:{:?}", e);
                        break;
                    }
                },
            }
        }
    }

    async fn background_io_incoming_single(
        socket: &Arc<UdpSocket>,
        recv_from_tx: &mpsc::Sender<MessagePair>,
        request_storage: &Arc<Mutex<OutboundRequestStorage>>,
    ) -> Result<(), RustyDHTError> {
        let mut buf = [0; 2048];
        let (num_bytes, sender) = socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| RustyDHTError::SocketRecvError(e.into()))?;
        trace!(target:"rustydht_lib::DHTSocket", "Receiving {} bytes from {}", num_bytes, sender);
        let message = packets::Message::from_bytes(&buf[..num_bytes])?;

        let request_info = {
            request_storage
                .lock()
                .unwrap()
                .take_matching_request_info(&message, sender)
        };
        // Is this message a reply to something we sent? If so, notify via specific channel
        if let Some(request_info) = request_info {
            if let Some(response_channel) = request_info.response_channel {
                if let Err(e) = response_channel.send(message.clone()).await {
                    let message = e.0;
                    warn!(target: "rustydht_lib::DHTSocket", "Got response, but sending code abandoned the channel receiver. So sad. Response: {:?}. Sender: {:?}", message, sender);
                }
            } else {
                warn!(target: "rustydht_lib::DHTSocket", "Got response, but can't notify due to no channel. I should make channel required. Response: {:?}. Sender: {:?}", message, sender);
            }
        }
        // Always send it to the generic recv_from channel
        recv_from_tx
            .send((message, sender))
            .await
            .map_err(|e| RustyDHTError::GeneralError(e.into()))?;
        Ok(())
    }

    async fn request_cleanup(request_storage: Arc<Mutex<OutboundRequestStorage>>) {
        let mut interval = interval(Duration::from_secs(10));

        loop {
            interval.tick().await;
            request_storage
                .lock()
                .unwrap()
                .prune_older_than(Duration::from_secs(10));
        }
    }
}

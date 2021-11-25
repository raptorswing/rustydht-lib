use crate::common::{Id, Node};
use crate::errors::RustyDHTError;
use crate::packets;
use crate::storage::outbound_request_storage::{OutboundRequestStorage, RequestInfo};
use smol::channel;
use smol::net::UdpSocket;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

pub struct DHTSocket {
    socket: UdpSocket,
    request_storage: OutboundRequestStorage,
}

impl DHTSocket {
    pub fn new(socket: UdpSocket) -> DHTSocket {
        DHTSocket {
            socket: socket,
            request_storage: OutboundRequestStorage::new(),
        }
    }

    pub async fn send_to(
        &mut self,
        msg: packets::Message,
        dest: SocketAddr,
        dest_id: Option<Id>,
    ) -> Result<channel::Receiver<packets::Message>, RustyDHTError> {
        let (sender, receiver) = channel::bounded(1);
        let request_info = RequestInfo::new(dest, dest_id, msg.clone(), Some(sender));
        self.request_storage.add_request(request_info);

        let bytes = msg.to_bytes()?;
        match self.socket.send_to(&bytes, dest).await {
            Ok(_) => Ok(receiver),
            Err(e) => {
                #[cfg(target_os = "linux")]
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    warn!(target: "rustydht_lib::DHTSocket", "send_to resulted in PermissionDenied. Is conntrack table full?");
                    return Err(RustyDHTError::ConntrackError(e.into()));
                }
                return Err(RustyDHTError::GeneralError(e.into()));
            }
        }
    }

    pub async fn recv_from(&mut self) -> Result<(packets::Message, SocketAddr), RustyDHTError> {
        let mut recv_buf = [0; 2048]; // All packets should fit within 1500 anyway
        let (num_read, src_addr) = self
            .socket
            .recv_from(&mut recv_buf)
            .await
            .map_err(|err| RustyDHTError::GeneralError(err.into()))?;
        let msg = packets::Message::from_bytes(&recv_buf[..num_read])?;

        if let packets::MessageType::Error(err_specific) = &msg.message_type {
            return Ok((msg, src_addr));
        }

        let sender_id = msg
            .get_author_id()
            .expect("Got packet without requester/responder id");
        // Was someone waiting for this specific message? Send it to them via fancy channel
        let matching_info = self.request_storage.take_matching_request_info(&msg);
        if let Some(matching_info) = matching_info {}

        // Otherwise, return it
        return Ok((msg, src_addr));
    }
}

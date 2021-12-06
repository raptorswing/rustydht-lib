use std::net::SocketAddr;

use crate::common::{Id, TransactionId};
use crate::packets::{Message, MessageType};

use std::time::{Duration, Instant};

use log::debug;
use tokio::sync::mpsc;

pub struct OutboundRequestStorage {
    requests: std::collections::HashMap<TransactionId, RequestInfo>,
}

impl OutboundRequestStorage {
    pub fn new() -> OutboundRequestStorage {
        OutboundRequestStorage {
            requests: std::collections::HashMap::new(),
        }
    }

    pub fn add_request(&mut self, info: RequestInfo) {
        self.requests
            .insert(info.packet.transaction_id.clone().into(), info);
    }

    #[cfg(test)]
    pub fn has_request<T>(&self, tid: &T) -> bool
    where
        T: Into<TransactionId>,
        T: Clone,
    {
        return self.requests.contains_key(&tid.clone().into());
    }

    pub fn get_matching_request_info(
        &self,
        msg: &Message,
        src_addr: SocketAddr,
    ) -> Option<&RequestInfo> {
        let tid = msg.transaction_id.clone().into();

        // Is this packet a response?
        if let MessageType::Response(res_specific) = &msg.message_type {
            // Is there a matching transaction id in storage?
            if let Some(request_info) = self.requests.get(&tid) {
                // Did this response come from the expected IP address?
                if request_info.addr == src_addr {
                    let response_sender_id = msg.get_author_id();
                    // Does the Id of the sender match the recorded addressee of the original request (if any)?
                    if request_info.id.is_none()
                        || (response_sender_id.is_some()
                            && request_info.id.unwrap() == response_sender_id.unwrap())
                    {
                        // Is the thing in storage a request packet (It should always be...)
                        if let MessageType::Request(req_specific) =
                            &request_info.packet.message_type
                        {
                            // Does the response type match the request type?
                            if crate::packets::response_matches_request(
                                &res_specific,
                                &req_specific,
                            ) {
                                return Some(request_info);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    pub fn take_matching_request_info(
        &mut self,
        response: &Message,
        src_addr: SocketAddr,
    ) -> Option<RequestInfo> {
        if let Some(_) = self.get_matching_request_info(response, src_addr) {
            let tid = response.transaction_id.clone().into();
            return self.requests.remove(&tid);
        }

        None
    }

    pub fn prune_older_than(&mut self, duration: Duration) {
        match Instant::now().checked_sub(duration) {
            None => {
                debug!(target: "rustydht_lib::OutboundRequestStorage",
                    "Outbound request storage skipping pruning due to monotonic clock underflow"
                );
            }

            Some(time) => {
                let len_before = self.requests.len();
                self.requests.retain(|_, v| -> bool {
                    return v.created_at >= time;
                });
                let len_after = self.requests.len();
                debug!(target: "rustydht_lib::OutboundRequestStorage", "Pruned {} request records", len_before - len_after);
            }
        }
    }

    pub fn len(&self) -> usize {
        return self.requests.len();
    }
}

#[derive(Debug)]
pub struct RequestInfo {
    addr: SocketAddr,
    id: Option<Id>,
    packet: Message,
    created_at: Instant,
    pub(crate) response_channel: Option<mpsc::Sender<Message>>,
}

impl RequestInfo {
    pub fn new(
        addr: SocketAddr,
        id: Option<Id>,
        packet: Message,
        response_channel: Option<mpsc::Sender<Message>>,
    ) -> RequestInfo {
        RequestInfo {
            addr: addr,
            id: id,
            packet: packet,
            created_at: Instant::now(),
            response_channel: response_channel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::packets::Message;

    #[test]
    fn test_outbound_request_storage() {
        let mut storage = OutboundRequestStorage::new();

        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let req = Message::create_ping_request(our_id);

        let request_target_addr = "127.0.0.1:1234".parse().unwrap();
        let request_info = RequestInfo::new(request_target_addr, None, req.clone(), None);

        // Add request to storage, make sure it's there
        storage.add_request(request_info);
        assert!(storage.has_request(&req.transaction_id));

        // Simulate a response, see if we correctly get the requet back from storage
        let simulated_response = Message::create_ping_response(
            our_id,
            req.transaction_id.clone(),
            "127.0.0.1:1235".parse().unwrap(),
        );

        // We should get something if the SocketAddr matches
        assert!(storage
            .get_matching_request_info(&simulated_response, request_target_addr)
            .is_some());

        // We should NOT get something if the SocketAddr doesn't match
        assert!(storage
            .get_matching_request_info(&simulated_response, "5.5.5.5:1234".parse().unwrap())
            .is_none());

        // Take the response
        assert!(storage
            .take_matching_request_info(&simulated_response, request_target_addr)
            .is_some());

        // Should have nothing left
        assert!(!storage.has_request(&req.transaction_id));
    }

    #[test]
    fn test_outbound_storage_prune() {
        let mut storage = OutboundRequestStorage::new();

        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let req = Message::create_ping_request(our_id);
        let req_2 = Message::create_ping_request(our_id);

        let request_info =
            RequestInfo::new("127.0.0.1:1234".parse().unwrap(), None, req.clone(), None);
        let mut request_info_2 =
            RequestInfo::new("127.0.0.1:1234".parse().unwrap(), None, req_2.clone(), None);
        request_info_2.created_at = Instant::now() + Duration::from_secs(10);

        // Add request to storage, make sure it's there
        storage.add_request(request_info);
        storage.add_request(request_info_2);
        assert!(storage.has_request(&req.transaction_id));

        // Prune, make sure request isn't there
        storage.prune_older_than(Duration::from_secs(0));
        assert!(!storage.has_request(&req.transaction_id));
        assert!(storage.has_request(&req_2.transaction_id));
    }
}

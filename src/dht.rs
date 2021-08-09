use rand::{thread_rng, Rng};

use smol::lock::Mutex;
use smol::net::UdpSocket;

extern crate crc;
use crc::{crc32, Hasher32};

use std::cell::RefCell;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant};

use crate::common::ipv4_addr_src::IPV4AddrSource;
use crate::common::{Id, Node};
use crate::errors::RustyDHTError;
use crate::packets;
use crate::storage::node_bucket_storage::NodeStorage;
use crate::storage::outbound_request_storage::OutboundRequestStorage;
use crate::storage::peer_storage::PeerStorage;
use crate::storage::throttler::Throttler;

pub struct DHTSettings {
    /// Number of bytes for token secrets for get_peers responses
    pub token_secret_size: usize,

    /// Max number of peers to provide in response to a get_peers.
    /// Shouldn't be much higher than this as the entire response packet needs to be less than 1500
    pub max_peers_response: usize,

    /// Max number of info hashes to provide in response to a sample_infohashes request
    pub max_sample_response: usize,

    /// How often we claim to rotate our sample_infohashes response
    pub min_sample_interval_secs: i32,

    /// We'll ping the "routers" at least this often (we may ping more frequently if needed)
    pub router_ping_interval_secs: u64,

    /// We'll ping previously-verified nodes at least this often to re-verify them
    pub reverify_interval_secs: u64,

    /// Verified nodes that we don't reverify within this amount of time are dropped
    pub reverify_grace_period_secs: u64,

    /// New nodes have this long to respond to a ping before we drop them
    pub verify_grace_period_secs: u64,

    /// When asked to provide peers, we'll only provide ones that announced within this time
    pub get_peers_freshness_secs: u64,

    /// We'll think about sending a find_nodes request at least this often.
    /// If we have enough nodes already we might not do it.
    pub find_nodes_interval_secs: u64,

    /// We won't send a periodic find_nodes request if we have at least this many unverified nodes
    pub find_nodes_skip_count: usize,

    /// Max number of torrents to store peers for
    pub max_torrents: usize,

    /// Max number of peers per torrent to store
    pub max_peers_per_torrent: usize,
}

impl DHTSettings {
    /// Returns DHTSettings with a default set of options.
    pub fn default() -> DHTSettings {
        DHTSettings {
            token_secret_size: 10,
            max_peers_response: 128,
            max_sample_response: 50,
            min_sample_interval_secs: 10,
            router_ping_interval_secs: 900,
            reverify_interval_secs: 14 * 60,
            reverify_grace_period_secs: 15 * 60,
            verify_grace_period_secs: 60,
            get_peers_freshness_secs: 15 * 60,
            find_nodes_interval_secs: 33,
            find_nodes_skip_count: 32,
            max_torrents: 50,
            max_peers_per_torrent: 100,
        }
    }
}

pub struct DHT {
    ip4_source: Mutex<Box<dyn IPV4AddrSource>>,
    our_id: RefCell<Id>,
    socket: UdpSocket,
    buckets: Mutex<Box<dyn NodeStorage>>,
    request_storage: Mutex<OutboundRequestStorage>,
    peer_storage: Mutex<PeerStorage>,
    token_secret: RefCell<Vec<u8>>,
    old_token_secret: RefCell<Vec<u8>>,
    routers: Vec<String>,
    settings: DHTSettings,
}

impl DHT {
    pub fn new<B>(
        id: Option<Id>,
        listen_port: u16,
        ip4_source: Box<dyn IPV4AddrSource>,
        buckets: B,
        routers: &[&str],
        settings: DHTSettings,
    ) -> Result<DHT, RustyDHTError>
    where
        B: FnOnce(Id) -> Box<dyn NodeStorage>,
    {
        // If we were given a hardcoded id, use that until/unless we decide its invalid based on IP source.
        // If we weren't given a hardcoded id, try to generate one based on IP source.
        // Finally, if all else fails, generate a totally random id.
        let our_id = {
            match id {
                Some(id) => id,

                None => match ip4_source.get_best_ipv4() {
                    Some(ip) => {
                        let id = Id::from_ip(&IpAddr::V4(ip));
                        eprintln!(
                            "Our external IPv4 is {:?}. Generated id {} based on that",
                            ip, id
                        );
                        id
                    }

                    None => {
                        let id = Id::from_random(&mut thread_rng());
                        eprintln!("No external IPv4 provided. Using random id {} for now.", id);
                        id
                    }
                },
            }
        };

        // Setup our UDP socket
        let socket = {
            let our_sockaddr =
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), listen_port));
            smol::block_on(UdpSocket::bind(our_sockaddr))
                .map_err(|err| RustyDHTError::GeneralError(err.into()))?
        };

        let mut token_secret = Vec::with_capacity(settings.token_secret_size);
        token_secret.fill_with(|| thread_rng().gen());

        Ok(DHT {
            ip4_source: Mutex::new(ip4_source),
            our_id: RefCell::new(our_id),
            socket: socket,
            buckets: Mutex::new(buckets(our_id)),
            request_storage: Mutex::new(OutboundRequestStorage::new()),
            peer_storage: Mutex::new(PeerStorage::new(
                settings.max_torrents,
                settings.max_peers_per_torrent,
            )),
            token_secret: RefCell::new(token_secret.clone()),
            old_token_secret: RefCell::new(token_secret),
            routers: routers.iter().map(|s| String::from(*s)).collect(),
            settings: settings,
        })
    }

    pub async fn accept_incoming_packets(&self) -> Result<(), RustyDHTError> {
        let mut throttler = Throttler::new(10, Duration::from_secs(6), Duration::from_secs(60));
        let mut recv_buf = [0; 2048]; // All packets should fit within 1500 anyway
        loop {
            match self
                .accept_single_packet(&mut throttler, &mut recv_buf)
                .await
            {
                Ok(_) => continue,

                Err(err) => match err {
                    RustyDHTError::PacketParseError(internal) => {
                        eprintln!("{:?}", internal);
                        continue;
                    }

                    RustyDHTError::GeneralError(_) => {
                        return Err(err.into());
                    }

                    RustyDHTError::PacketSerializationError(_) => {
                        return Err(err.into());
                    }
                },
            }
        }
    }

    async fn accept_single_packet(
        &self,
        throttler: &mut Throttler<32>,
        recv_buf: &mut [u8; 2048],
    ) -> Result<(), RustyDHTError> {
        let (num_read, addr) = self
            .socket
            .recv_from(recv_buf)
            .await
            .map_err(|err| RustyDHTError::GeneralError(err.into()))?;
        let msg = packets::Message::from_bytes(&recv_buf[..num_read])?;

        if throttler.check_throttle(addr.ip(), None) {
            eprintln!("{} is rate limited - dropping packet", addr.ip());
            return Ok(());
        }

        if addr.port() == 0 {
            eprintln!("{} has invalid port - dropping packet", addr);
            return Ok(());
        }

        match &msg.message_type {
            packets::MessageType::Request(request_variant) => {
                match request_variant {
                    packets::RequestSpecific::PingRequest(arguments) => {
                        // Is id valid for IP?
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        if is_id_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        // Build a ping reply
                        let reply = packets::Message::create_ping_response(
                            *self.our_id.borrow(),
                            msg.transaction_id.clone(),
                            addr,
                        );
                        let reply_bytes = reply.to_bytes()?;
                        self.send_to(&reply_bytes, addr).await?;
                    }

                    packets::RequestSpecific::GetPeersRequest(arguments) => {
                        // Is id valid for IP?
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        if is_id_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        // First, see if we have any peers for their info_hash
                        let peers = {
                            let peer_storage = self.peer_storage.lock().await;
                            let newer_than = Instant::now().checked_sub(Duration::from_secs(
                                self.settings.get_peers_freshness_secs,
                            ));
                            let mut peers =
                                peer_storage.get_peers(&arguments.info_hash, newer_than);
                            peers.truncate(self.settings.max_peers_response);
                            peers
                        };
                        let token = calculate_token(&addr, self.token_secret.borrow().clone());

                        let reply = match peers.len() {
                            0 => {
                                let buckets = self.buckets.lock().await;
                                let nearest = buckets.get_nearest_nodes(
                                    &arguments.info_hash,
                                    Some(&arguments.requester_id),
                                );

                                packets::Message::create_get_peers_response_no_peers(
                                    self.our_id.borrow().clone(),
                                    msg.transaction_id,
                                    addr,
                                    token.to_vec(),
                                    nearest.iter().map(|&node| node.clone()).collect(),
                                )
                            }

                            _ => packets::Message::create_get_peers_response_peers(
                                self.our_id.borrow().clone(),
                                msg.transaction_id,
                                addr,
                                token.to_vec(),
                                peers,
                            ),
                        };

                        let reply_bytes = reply.to_bytes()?;
                        self.send_to(&reply_bytes, addr).await?;
                    }

                    packets::RequestSpecific::FindNodeRequest(arguments) => {
                        // Is id valid for IP?
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        if is_id_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        // We're fine to respond regardless
                        let buckets = self.buckets.lock().await;
                        let nearest = buckets
                            .get_nearest_nodes(&arguments.target, Some(&arguments.requester_id))
                            .iter()
                            .map(|&n_ref| n_ref.clone())
                            .collect();

                        let reply = packets::Message::create_find_node_response(
                            self.our_id.borrow().clone(),
                            msg.transaction_id,
                            addr,
                            nearest,
                        );
                        let reply_bytes = reply.to_bytes()?;
                        self.send_to(&reply_bytes, addr).await?;
                    }

                    packets::RequestSpecific::AnnouncePeerRequest(arguments) => {
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());

                        let is_token_valid = arguments.token
                            == calculate_token(&addr, self.token_secret.borrow().clone())
                            || arguments.token
                                == calculate_token(&addr, self.old_token_secret.borrow().clone());

                        if is_id_valid {
                            if is_token_valid {
                                self.buckets
                                    .lock()
                                    .await
                                    .add_or_update(Node::new(arguments.requester_id, addr), true);
                            } else {
                                self.buckets
                                    .lock()
                                    .await
                                    .add_or_update(Node::new(arguments.requester_id, addr), false);
                            }
                        }

                        if is_token_valid {
                            let sockaddr = match arguments.implied_port {
                                Some(implied_port) if implied_port == true => addr,

                                _ => {
                                    let mut tmp = addr.clone();
                                    tmp.set_port(arguments.port);
                                    tmp
                                }
                            };

                            self.peer_storage
                                .lock()
                                .await
                                .announce_peer(arguments.info_hash, sockaddr);

                            // Response is same for ping, so reuse that
                            let reply = packets::Message::create_ping_response(
                                *self.our_id.borrow(),
                                msg.transaction_id.clone(),
                                addr,
                            );
                            let reply_bytes = reply.to_bytes()?;
                            self.send_to(&reply_bytes, addr).await?;
                        }
                    }

                    // super::packets::KRPCRequestSpecific::KRPCSampleInfoHashesRequest {
                    //     arguments,
                    // } => {
                    //     let target = MainlineId::from_bytes(&arguments.target)?;

                    //     let id = MainlineId::from_bytes(&arguments.id)?;
                    //     let is_id_valid = id.is_valid_for_ip(&addr.ip());
                    //     let mut buckets = self.buckets.lock().await;
                    //     if is_id_valid {
                    //         buckets.add_or_update(Node::new(id, addr), false);
                    //     }

                    //     let nearest = buckets.get_nearest_nodes(&target, Some(&id));
                    //     let info_hashes = {
                    //         let mut rng = thread_rng();
                    //         let peer_storage = self.peer_storage.lock().await;
                    //         peer_storage
                    //             .get_info_hashes()
                    //             .as_mut_slice()
                    //             .partial_shuffle(&mut rng, MAX_SAMPLE_RESPONSE)
                    //             .0
                    //             .to_vec()
                    //     };
                    //     let reply = super::packets::create_sample_infohashes_response(
                    //         &self.our_id.borrow(),
                    //         msg.transaction_id,
                    //         &addr,
                    //         MIN_SAMPLE_INTERVAL_SECS,
                    //         &nearest,
                    //         &info_hashes,
                    //     );
                    //     let reply_bytes = serde_bencode::to_bytes(&reply)
                    //         .expect("Failed to serialize sample_infohashes response");
                    //     send_to!(self.socket, &reply_bytes, addr);
                    // }
                    _ => {
                        eprintln!("Received unimplemented request type");
                    }
                }
            }

            packets::MessageType::Response(response_variant) => {
                match response_variant {
                    // super::packets::KRPCResponseSpecific::KRPCPingResponse { arguments } => {
                    //     let id = MainlineId::from_bytes(&arguments.id)?;

                    //     let is_id_valid = id.is_valid_for_ip(&addr.ip());
                    //     if !is_id_valid {
                    //         return Ok(());
                    //     }

                    //     if let IpAddr::V4(their_ip) = addr.ip() {
                    //         if let Some(they_claim_our_sockaddr) = &msg.ip {
                    //             let they_claim_our_sockaddr =
                    //                 super::packets::bytes_to_sockaddrv4(they_claim_our_sockaddr)?;
                    //             self.ip4_source
                    //                 .lock()
                    //                 .await
                    //                 .add_vote(their_ip, they_claim_our_sockaddr.ip().clone());
                    //         }
                    //     }

                    //     let mut buckets = self.buckets.lock().await;
                    //     let mut request_storage = self.request_storage.lock().await;

                    //     if request_storage.take_matching_request_info(&msg).is_some() {
                    //         buckets.add_or_update(Node::new(id, addr), true);
                    //     }
                    // }

                    // super::packets::KRPCResponseSpecific::KRPCFindNodeResponse { arguments } => {
                    //     let their_id = MainlineId::from_bytes(&arguments.id)?;

                    //     if let IpAddr::V4(their_ip) = addr.ip() {
                    //         if let Some(they_claim_our_sockaddr) = &msg.ip {
                    //             let they_claim_our_sockaddr =
                    //                 super::packets::bytes_to_sockaddrv4(they_claim_our_sockaddr)?;
                    //             self.ip4_source
                    //                 .lock()
                    //                 .await
                    //                 .add_vote(their_ip, they_claim_our_sockaddr.ip().clone());
                    //         }
                    //     }

                    //     let mut buckets = self.buckets.lock().await;
                    //     let mut request_storage = self.request_storage.lock().await;

                    //     if request_storage.take_matching_request_info(&msg).is_some() {
                    //         buckets.add_or_update(Node::new(their_id, addr), true);

                    //         // Add the nodes we got back as "seen" (even though we haven't necessarily seen them directly yet).
                    //         // We'll ping them and try to verify them
                    //         let nodes = super::packets::bytes_to_nodes(&arguments.nodes)?;
                    //         for node in nodes {
                    //             if node.id.is_valid_for_ip(&node.address.ip()) {
                    //                 buckets.add_or_update(node, false);
                    //             }
                    //         }
                    //     } else {
                    //         eprintln!("Ignoring unsolicited find_node response");
                    //     }
                    // }
                    _ => {
                        eprintln!(
                            "Received unsupported/unexpected KRPCResponse variant from {:?}: {:?}",
                            addr, response_variant
                        );
                    }
                }
            }
            _ => {
                eprintln!(
                    "Received unsupported/unexpected KRPCMessage variant from {:?}: {:?}",
                    addr, msg
                );
            }
        }
        return Ok(());
    }

    pub fn get_id(&self) -> Id {
        self.our_id.borrow().clone()
    }

    async fn send_to(&self, bytes: &Vec<u8>, dest: SocketAddr) -> Result<(), RustyDHTError> {
        self.socket
            .send_to(bytes, dest)
            .await
            .map_err(|err| RustyDHTError::GeneralError(err.into()))?;
        Ok(())
    }
}

/// Calculates a peer announce token based on a sockaddr and some secret.
/// Pretty positive this isn't cryptographically safe but I'm not too worried.
/// If we care about that later we can use a proper HMAC or something.
fn calculate_token<T: AsRef<[u8]>>(remote: &SocketAddr, secret: T) -> [u8; 4] {
    let secret = secret.as_ref();
    let mut digest = crc32::Digest::new(crc32::CASTAGNOLI);
    // digest.write(&crate::packets::sockaddr_to_bytes(remote));
    let octets = match remote.ip() {
        std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
        std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
    };
    digest.write(&octets);
    digest.write(secret);
    let checksum: u32 = digest.sum32();

    return checksum.to_be_bytes();
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::common::ipv4_addr_src::StaticIPV4AddrSource;
    use lazy_static::lazy_static;
    use std::boxed::Box;

    // Tests reuse the same UDP port. This mutex is used to serialize tests that need the UDP port.
    lazy_static! {
        static ref LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }

    #[test]
    fn test_responds_to_ping() -> Result<(), RustyDHTError> {
        smol::block_on(async {
            let requester_id = Id::from_random(&mut thread_rng());
            let ping_request = packets::Message::create_ping_request(requester_id);

            let res = futures::try_join!(
                accept_single_packet(),
                send_and_receive(ping_request.clone()),
            )
            .map(|res| res.1)?;

            assert_eq!(res.transaction_id, ping_request.transaction_id);
            assert_eq!(
                res.message_type,
                packets::MessageType::Response(packets::ResponseSpecific::PingResponse(
                    packets::PingResponseArguments {
                        responder_id: get_dht_id()
                    }
                ))
            );

            Ok(())
        })
    }

    #[test]
    fn test_responds_to_get_peers() -> Result<(), RustyDHTError> {
        smol::block_on(async {
            let requester_id = Id::from_random(&mut thread_rng());
            let desired_info_hash = Id::from_random(&mut thread_rng());
            let request =
                packets::Message::create_get_peers_request(requester_id, desired_info_hash);

            let res = futures::try_join!(accept_single_packet(), send_and_receive(request.clone()))
                .map(|res| res.1)?;

            assert_eq!(res.transaction_id, request.transaction_id);
            assert!(matches!(
                res.message_type,
                packets::MessageType::Response(packets::ResponseSpecific::GetPeersResponse(
                    packets::GetPeersResponseArguments { .. }
                ))
            ));

            Ok(())
        })
    }

    #[test]
    fn test_responds_to_find_node() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let target = Id::from_random(&mut thread_rng());
        smol::block_on(async {
            let request = packets::Message::create_find_node_request(requester_id, target);

            let res = futures::try_join!(accept_single_packet(), send_and_receive(request.clone()))
                .map(|res| res.1)?;

            assert_eq!(res.transaction_id, request.transaction_id);
            assert!(matches!(
                res.message_type,
                packets::MessageType::Response(packets::ResponseSpecific::FindNodeResponse(
                    packets::FindNodeResponseArguments { .. }
                ))
            ));

            Ok(())
        })
    }

    // Dumb helper function because we can't declare a const or static Id
    fn get_dht_id() -> Id {
        Id::from_hex("0011223344556677889900112233445566778899").unwrap()
    }

    // Helper function that creates a test DHT, handles a single packet, and returns
    async fn accept_single_packet() -> Result<(), RustyDHTError> {
        let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
        let phony_ip4 = Box::new(StaticIPV4AddrSource::new(ipv4));
        let buckets = |id| -> Box<dyn NodeStorage> {
            Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                id, 8,
            ))
        };
        let _lock = LOCK.lock();
        let dht = DHT::new(
            Some(get_dht_id()),
            10001,
            phony_ip4,
            buckets,
            &[],
            DHTSettings::default(),
        )
        .unwrap();

        let mut throttler = crate::storage::throttler::Throttler::new(
            100,
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let mut recv_buf = [0; 2048];

        dht.accept_single_packet(&mut throttler, &mut recv_buf)
            .await
    }

    // Helper function that sends a single packet to the test DHT and then returns the response
    async fn send_and_receive(msg: packets::Message) -> Result<packets::Message, RustyDHTError> {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.send_to(&msg.clone().to_bytes().unwrap(), "127.0.0.1:10001")
            .await
            .unwrap();
        let mut recv_buf = [0; 2048];
        let num_read = sock.recv_from(&mut recv_buf).await.unwrap().0;
        packets::Message::from_bytes(&recv_buf[..num_read])
    }
}
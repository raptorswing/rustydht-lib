use rand::{thread_rng, Rng, RngCore};

use smol::lock::Mutex;
use smol::net::UdpSocket;

use std::cell::RefCell;
use std::net::{IpAddr, SocketAddr, SocketAddrV4, Ipv4Addr};
use std::time::{Duration, Instant};
use std::ops::Sub;

use crate::common::{Id, Node};
use crate::storage::peer_storage::PeerStorage;
use crate::storage::node_bucket_storage::NodeStorage;
use crate::storage::outbound_request_storage::OutboundRequestStorage;
use crate::storage::throttler::Throttler;
use crate::common::ipv4_addr_src::IPV4AddrSource;
use crate::errors::RustyDHTError;
use crate::packets;

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
            reverify_interval_secs: 14*60,
            reverify_grace_period_secs: 15*60,
            verify_grace_period_secs: 60,
            get_peers_freshness_secs: 15*60,
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
    settings: DHTSettings
}

impl DHT {
    pub fn new(
        listen_port: u16,
        ip4_source: Box<dyn IPV4AddrSource>,
        buckets: Box<dyn NodeStorage>,
        routers: &[&str],
        settings: DHTSettings,
    ) -> Result<DHT, RustyDHTError> {
        // Decide on an initial node id based on best current guess of our IPv4 address.
        // It's OK if we don't know yet, we'll change our id later if needed.
        let our_id = match ip4_source.get_best_ipv4() {
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
        };

        // Setup our UDP socket
        let socket = {
            let our_sockaddr =
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), listen_port));
            smol::block_on(UdpSocket::bind(our_sockaddr)).map_err(|err| RustyDHTError::GeneralError(err.into()))?
        };

        let mut token_secret = Vec::with_capacity(settings.token_secret_size);
        token_secret.fill_with(|| thread_rng().gen());

        Ok(DHT {
            ip4_source: Mutex::new(ip4_source),
            our_id: RefCell::new(our_id),
            socket: socket,
            buckets: Mutex::new(buckets),
            request_storage: Mutex::new(OutboundRequestStorage::new()),
            peer_storage: Mutex::new(PeerStorage::new(settings.max_torrents, settings.max_peers_per_torrent)),
            token_secret: RefCell::new(token_secret.clone()),
            old_token_secret: RefCell::new(token_secret),
            routers: routers.iter().map(|s| String::from(*s)).collect(),
            settings: DHTSettings,
        })
    }

    async fn accept_incoming_packets(&self) -> Result<(), RustyDHTError> {
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
                    packets::RequestSpecific::PingRequest ( arguments ) => {
                        // Is id valid for IP?
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        if is_id_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        // Build a ping reply
                        let reply = packets::create_ping_response(
                            *self.our_id.borrow(),
                            msg.transaction_id.clone(),
                            addr);
  
                        let reply_bytes = reply.to_bytes()?;
                        self.socket.send_to(&reply_bytes, addr);
                    }

                    packets::RequestSpecific::GetPeersRequest ( arguments ) => {
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
                            peer_storage.get_peers(
                                &arguments.info_hash,
                                Some(
                                    Instant::now()
                                        .sub(Duration::from_secs(self.settings.get_peers_freshness_secs)),
                                ),
                            )
                        };
                        let token =
                            super::packets::calculate_token(&addr, *self.token_secret.borrow());

                        let reply = match peers.len() {
                            0 => {
                                let buckets = self.buckets.lock().await;
                                let nearest = buckets.get_nearest_nodes(&info_hash, Some(&id));

                                super::packets::create_get_peers_response_no_peers(
                                    &self.our_id.borrow(),
                                    msg.transaction_id,
                                    &addr,
                                    token.to_vec(),
                                    &nearest,
                                )
                            }

                            _ => super::packets::create_get_peers_response_peers(
                                &self.our_id.borrow(),
                                msg.transaction_id,
                                &addr,
                                token.to_vec(),
                                &peers[0..std::cmp::min(MAX_PEERS_RESPONSE, peers.len())],
                            ),
                        };

                        let reply_bytes = serde_bencode::to_bytes(&reply)
                            .expect("Failed to prepare get_peers reply");
                        send_to!(self.socket, &reply_bytes, addr);
                    }

                    super::packets::KRPCRequestSpecific::KRPCFindNodeRequest { arguments } => {
                        // Is id valid for IP?
                        let id = MainlineId::from_bytes(&arguments.id)?;
                        let is_id_valid = id.is_valid_for_ip(&addr.ip());
                        if is_id_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(super::Node::new(id, addr), false);
                        }

                        let target = MainlineId::from_bytes(&arguments.target)?;

                        // We're fine to respond regardless
                        let buckets = self.buckets.lock().await;
                        let nearest = buckets.get_nearest_nodes(&target, Some(&id));

                        let reply = super::packets::create_find_node_response(
                            &self.our_id.borrow(),
                            msg.transaction_id,
                            &addr,
                            &nearest,
                        );
                        let reply_bytes = serde_bencode::to_bytes(&reply)
                            .expect("Failed to prepare find_node reply");
                        send_to!(self.socket, &reply_bytes, addr);
                    }

                    super::packets::KRPCRequestSpecific::KRPCAnnouncePeerRequest { arguments } => {
                        let id = MainlineId::from_bytes(&arguments.id)?;
                        let is_id_valid = id.is_valid_for_ip(&addr.ip());

                        let info_hash = MainlineId::from_bytes(&arguments.info_hash)?;

                        let is_token_valid = arguments.token
                            == super::packets::calculate_token(&addr, *self.token_secret.borrow())
                            || arguments.token
                                == super::packets::calculate_token(
                                    &addr,
                                    *self.old_token_secret.borrow(),
                                );

                        if is_id_valid {
                            if is_token_valid {
                                self.buckets
                                    .lock()
                                    .await
                                    .add_or_update(super::Node::new(id, addr), true);
                            } else {
                                self.buckets
                                    .lock()
                                    .await
                                    .add_or_update(super::Node::new(id, addr), false);
                            }
                        }

                        if is_token_valid {
                            self.buckets
                                .lock()
                                .await
                                .add_or_update(super::Node::new(id, addr), true);

                            let sockaddr = match arguments.implied_port {
                                Some(byte) if byte != 0x0 => addr,

                                _ => {
                                    let mut tmp = addr.clone();
                                    tmp.set_port(arguments.port);
                                    tmp
                                }
                            };

                            self.peer_storage
                                .lock()
                                .await
                                .announce_peer(&info_hash, sockaddr);

                            // Response is same for ping, so reuse that
                            let reply = super::packets::create_ping_response(
                                &self.our_id.borrow(),
                                msg.transaction_id,
                                &addr,
                            );
                            let reply_bytes = serde_bencode::to_bytes(&reply)
                                .expect("Failed to prepare find_node reply");
                            send_to!(self.socket, &reply_bytes, addr);
                        }
                    }

                    super::packets::KRPCRequestSpecific::KRPCSampleInfoHashesRequest {
                        arguments,
                    } => {
                        let target = MainlineId::from_bytes(&arguments.target)?;

                        let id = MainlineId::from_bytes(&arguments.id)?;
                        let is_id_valid = id.is_valid_for_ip(&addr.ip());
                        let mut buckets = self.buckets.lock().await;
                        if is_id_valid {
                            buckets.add_or_update(super::Node::new(id, addr), false);
                        }

                        let nearest = buckets.get_nearest_nodes(&target, Some(&id));
                        let info_hashes = {
                            let mut rng = thread_rng();
                            let peer_storage = self.peer_storage.lock().await;
                            peer_storage
                                .get_info_hashes()
                                .as_mut_slice()
                                .partial_shuffle(&mut rng, MAX_SAMPLE_RESPONSE)
                                .0
                                .to_vec()
                        };
                        let reply = super::packets::create_sample_infohashes_response(
                            &self.our_id.borrow(),
                            msg.transaction_id,
                            &addr,
                            MIN_SAMPLE_INTERVAL_SECS,
                            &nearest,
                            &info_hashes,
                        );
                        let reply_bytes = serde_bencode::to_bytes(&reply)
                            .expect("Failed to serialize sample_infohashes response");
                        send_to!(self.socket, &reply_bytes, addr);
                    }
                }
            }

            super::packets::KRPCMessageVariant::KRPCResponse(response_variant) => {
                match response_variant {
                    super::packets::KRPCResponseSpecific::KRPCPingResponse { arguments } => {
                        let id = MainlineId::from_bytes(&arguments.id)?;

                        let is_id_valid = id.is_valid_for_ip(&addr.ip());
                        if !is_id_valid {
                            return Ok(());
                        }

                        if let IpAddr::V4(their_ip) = addr.ip() {
                            if let Some(they_claim_our_sockaddr) = &msg.ip {
                                let they_claim_our_sockaddr =
                                    super::packets::bytes_to_sockaddrv4(they_claim_our_sockaddr)?;
                                self.ip4_source
                                    .lock()
                                    .await
                                    .add_vote(their_ip, they_claim_our_sockaddr.ip().clone());
                            }
                        }

                        let mut buckets = self.buckets.lock().await;
                        let mut request_storage = self.request_storage.lock().await;

                        if request_storage.take_matching_request_info(&msg).is_some() {
                            buckets.add_or_update(super::Node::new(id, addr), true);
                        }
                    }

                    super::packets::KRPCResponseSpecific::KRPCFindNodeResponse { arguments } => {
                        let their_id = MainlineId::from_bytes(&arguments.id)?;

                        if let IpAddr::V4(their_ip) = addr.ip() {
                            if let Some(they_claim_our_sockaddr) = &msg.ip {
                                let they_claim_our_sockaddr =
                                    super::packets::bytes_to_sockaddrv4(they_claim_our_sockaddr)?;
                                self.ip4_source
                                    .lock()
                                    .await
                                    .add_vote(their_ip, they_claim_our_sockaddr.ip().clone());
                            }
                        }

                        let mut buckets = self.buckets.lock().await;
                        let mut request_storage = self.request_storage.lock().await;

                        if request_storage.take_matching_request_info(&msg).is_some() {
                            buckets.add_or_update(super::Node::new(their_id, addr), true);

                            // Add the nodes we got back as "seen" (even though we haven't necessarily seen them directly yet).
                            // We'll ping them and try to verify them
                            let nodes = super::packets::bytes_to_nodes(&arguments.nodes)?;
                            for node in nodes {
                                if node.id.is_valid_for_ip(&node.address.ip()) {
                                    buckets.add_or_update(node, false);
                                }
                            }
                        } else {
                            eprintln!("Ignoring unsolicited find_node response");
                        }
                    }

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
}
use anyhow::anyhow;

use rand::prelude::SliceRandom;
use rand::{thread_rng, Rng};

use futures::StreamExt;
use tokio::net::{lookup_host, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::sleep;

use log::{debug, info, trace, warn};

extern crate crc;
use crc::{crc32, Hasher32};

use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::common::ipv4_addr_src::IPV4AddrSource;
use crate::common::{Id, Node};
use crate::dht_event::{DHTEvent, DHTEventType, MessageReceivedEvent};
use crate::errors::RustyDHTError;
use crate::packets;
use crate::shutdown;
use crate::socket::DHTSocket;
use crate::storage::node_bucket_storage::NodeStorage;
use crate::storage::peer_storage::{PeerInfo, PeerStorage};
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

    /// We'll think about pinging and pruning nodes at this interval
    pub ping_check_interval_secs: u64,

    /// Outgoing requests may be pruned after this many seconds
    pub outgoing_request_prune_secs: u64,

    /// We'll think about pruning outgoing requests at this interval
    pub outgoing_reqiest_check_interval_secs: u64,
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
            ping_check_interval_secs: 10,
            outgoing_request_prune_secs: 30,
            outgoing_reqiest_check_interval_secs: 30,
        }
    }
}

struct DHTState {
    ip4_source: Box<dyn IPV4AddrSource + Send>,
    our_id: Id,
    buckets: Box<dyn NodeStorage + Send>,
    peer_storage: PeerStorage,
    token_secret: Vec<u8>,
    old_token_secret: Vec<u8>,
    routers: Vec<String>,
    settings: DHTSettings,
    subscribers: Vec<mpsc::Sender<DHTEvent>>,
}

pub struct DHT {
    socket: Arc<DHTSocket>,

    // Coarse-grained locking for stuff what needs it
    state: Arc<Mutex<DHTState>>,

    shutdown: shutdown::ShutdownReceiver,
}

impl DHT {
    pub async fn new<B>(
        shutdown: shutdown::ShutdownReceiver,
        id: Option<Id>,
        listen_port: u16,
        ip4_source: Box<dyn IPV4AddrSource + Send>,
        buckets: B,
        routers: &[&str],
        settings: DHTSettings,
    ) -> Result<DHT, RustyDHTError>
    where
        B: FnOnce(Id) -> Box<dyn NodeStorage + Send>,
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
                        info!(target: "rustydht_lib::DHT",
                            "Our external IPv4 is {:?}. Generated id {} based on that",
                            ip, id
                        );
                        id
                    }

                    None => {
                        let id = Id::from_random(&mut thread_rng());
                        info!(target: "rustydht_lib::DHT", "No external IPv4 provided. Using random id {} for now.", id);
                        id
                    }
                },
            }
        };

        // Setup our UDP socket
        let socket = {
            let our_sockaddr =
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), listen_port));
            UdpSocket::bind(our_sockaddr)
                .await
                .map_err(|e| RustyDHTError::GeneralError(e.into()))?
        };
        let socket = Arc::new(DHTSocket::new(shutdown.clone(), socket));

        let token_secret = make_token_secret(settings.token_secret_size);

        let dht = DHT {
            socket: socket,
            state: Arc::new(Mutex::new(DHTState {
                ip4_source: ip4_source,
                our_id: our_id,
                buckets: buckets(our_id),
                peer_storage: PeerStorage::new(
                    settings.max_torrents,
                    settings.max_peers_per_torrent,
                ),
                token_secret: token_secret.clone(),
                old_token_secret: token_secret,
                routers: routers.iter().map(|s| String::from(*s)).collect(),
                settings: settings,
                subscribers: vec![],
            })),

            shutdown: shutdown,
        };

        Ok(dht)
    }

    async fn accept_incoming_packets(&self) -> Result<(), RustyDHTError> {
        let mut throttler = Throttler::new(
            10,
            Duration::from_secs(6),
            Duration::from_secs(60),
            Duration::from_secs(86400),
        );
        loop {
            match self.accept_single_packet(&mut throttler).await {
                Ok(_) => continue,

                Err(err) => match err {
                    RustyDHTError::PacketParseError(internal) => {
                        warn!(target: "rustydht_lib::DHT", "Packet parsing error: {:?}", internal);
                        continue;
                    }

                    RustyDHTError::ConntrackError(e) => {
                        warn!(target: "rustydht_lib::DHT", "Connection tracking error: {:?}", e);
                        continue;
                    }

                    _ => {
                        return Err(err.into());
                    }
                },
            }
        }
    }

    async fn accept_single_packet(
        &self,
        throttler: &mut Throttler<32>,
    ) -> Result<(), RustyDHTError> {
        let (msg, addr) = self.socket.recv_from().await?;

        // Drop the packet if the IP has been throttled.
        if throttler.check_throttle(addr.ip(), None, None) {
            return Ok(());
        }

        // Filter out packets sent from port 0. We can't reply to these.
        if addr.port() == 0 {
            warn!(target: "rustydht_lib::DHT", "{} has invalid port - dropping packet", addr);
            return Ok(());
        }

        // We'll use this clone to send an event later.
        // "It's a surprise tool that will help us later!"
        let msg_event_clone = msg.clone();

        match &msg.message_type {
            packets::MessageType::Request(request_variant) => {
                match request_variant {
                    packets::RequestSpecific::PingRequest(arguments) => {
                        // Is id valid for IP?
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        let read_only = match msg.read_only {
                            Some(ro) => ro,
                            _ => false,
                        };
                        if is_id_valid && !read_only {
                            self.state
                                .try_lock()
                                .unwrap()
                                .buckets
                                .add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        // Build a ping reply
                        let reply = packets::Message::create_ping_response(
                            self.state.try_lock().unwrap().our_id,
                            msg.transaction_id.clone(),
                            addr,
                        );
                        self.socket
                            .send_to(reply, addr, Some(arguments.requester_id))
                            .await?;
                    }

                    packets::RequestSpecific::GetPeersRequest(arguments) => {
                        let reply = {
                            let mut state = self.state.try_lock().unwrap();
                            // Is id valid for IP?
                            let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                            let read_only = match msg.read_only {
                                Some(ro) => ro,
                                _ => false,
                            };
                            if is_id_valid && !read_only {
                                state
                                    .buckets
                                    .add_or_update(Node::new(arguments.requester_id, addr), false);
                            }

                            // First, see if we have any peers for their info_hash
                            let peers = {
                                let newer_than = Instant::now().checked_sub(Duration::from_secs(
                                    state.settings.get_peers_freshness_secs,
                                ));
                                let mut peers = state
                                    .peer_storage
                                    .get_peers(&arguments.info_hash, newer_than);
                                peers.truncate(state.settings.max_peers_response);
                                peers
                            };
                            let token = calculate_token(&addr, state.token_secret.clone());

                            let reply = match peers.len() {
                                0 => {
                                    let nearest = state.buckets.get_nearest_nodes(
                                        &arguments.info_hash,
                                        Some(&arguments.requester_id),
                                    );

                                    packets::Message::create_get_peers_response_no_peers(
                                        state.our_id.clone(),
                                        msg.transaction_id,
                                        addr,
                                        token.to_vec(),
                                        nearest,
                                    )
                                }

                                _ => packets::Message::create_get_peers_response_peers(
                                    state.our_id.clone(),
                                    msg.transaction_id,
                                    addr,
                                    token.to_vec(),
                                    peers,
                                ),
                            };
                            reply
                        };
                        self.socket
                            .send_to(reply, addr, Some(arguments.requester_id))
                            .await?;
                    }

                    packets::RequestSpecific::FindNodeRequest(arguments) => {
                        let reply = {
                            let mut state = self.state.try_lock().unwrap();
                            // Is id valid for IP?
                            let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                            let read_only = match msg.read_only {
                                Some(ro) => ro,
                                _ => false,
                            };
                            if is_id_valid && !read_only {
                                state
                                    .buckets
                                    .add_or_update(Node::new(arguments.requester_id, addr), false);
                            }
                            // We're fine to respond regardless
                            let nearest = state.buckets.get_nearest_nodes(
                                &arguments.target,
                                Some(&arguments.requester_id),
                            );
                            packets::Message::create_find_node_response(
                                state.our_id.clone(),
                                msg.transaction_id,
                                addr,
                                nearest,
                            )
                        };

                        self.socket
                            .send_to(reply, addr, Some(arguments.requester_id))
                            .await?;
                    }

                    packets::RequestSpecific::AnnouncePeerRequest(arguments) => {
                        let reply = {
                            let mut state = self.state.try_lock().unwrap();
                            let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                            let read_only = match msg.read_only {
                                Some(ro) => ro,
                                _ => false,
                            };

                            let is_token_valid = arguments.token
                                == calculate_token(&addr, state.token_secret.clone())
                                || arguments.token
                                    == calculate_token(&addr, state.old_token_secret.clone());

                            if is_id_valid && !read_only {
                                state.buckets.add_or_update(
                                    Node::new(arguments.requester_id, addr),
                                    is_token_valid,
                                );
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

                                state
                                    .peer_storage
                                    .announce_peer(arguments.info_hash, sockaddr);

                                // Response is same for ping, so reuse that
                                Some(packets::Message::create_ping_response(
                                    state.our_id,
                                    msg.transaction_id.clone(),
                                    addr,
                                ))
                            } else {
                                None
                            }
                        };

                        if let Some(reply) = reply {
                            self.socket
                                .send_to(reply, addr, Some(arguments.requester_id))
                                .await?;
                        }
                    }

                    packets::RequestSpecific::SampleInfoHashesRequest(arguments) => {
                        let reply = {
                            let mut state = self.state.try_lock().unwrap();
                            let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                            let read_only = match msg.read_only {
                                Some(ro) => ro,
                                _ => false,
                            };
                            if is_id_valid && !read_only {
                                state
                                    .buckets
                                    .add_or_update(Node::new(arguments.requester_id, addr), false);
                            }

                            let nearest = state.buckets.get_nearest_nodes(
                                &arguments.target,
                                Some(&arguments.requester_id),
                            );

                            let (info_hashes, total_info_hashes) = {
                                let info_hashes = state.peer_storage.get_info_hashes();
                                let total_info_hashes = info_hashes.len();
                                let info_hashes = {
                                    let mut rng = thread_rng();
                                    state
                                        .peer_storage
                                        .get_info_hashes()
                                        .as_mut_slice()
                                        .partial_shuffle(
                                            &mut rng,
                                            state.settings.max_sample_response,
                                        )
                                        .0
                                        .to_vec()
                                };
                                (info_hashes, total_info_hashes)
                            };

                            packets::Message::create_sample_infohashes_response(
                                state.our_id,
                                msg.transaction_id,
                                addr,
                                Duration::from_secs(
                                    state.settings.min_sample_interval_secs.try_into().unwrap(),
                                ),
                                nearest,
                                info_hashes,
                                total_info_hashes,
                            )
                        };

                        self.socket
                            .send_to(reply, addr, Some(arguments.requester_id))
                            .await?;
                    }
                }
            }

            packets::MessageType::Response(response_variant) => match response_variant {
                _ => { /*Responses should be handled by the sender via notification channel.*/ }
            },
            _ => {
                warn!(target: "rustydht_lib::DHT",
                    "Received unsupported/unexpected KRPCMessage variant from {:?}: {:?}",
                    addr, msg
                );
            }
        }

        {
            // Notify any subscribers about the event
            let event = DHTEvent {
                event_type: DHTEventType::MessageReceived(MessageReceivedEvent {
                    message: msg_event_clone,
                }),
            };
            let mut state = self.state.lock().unwrap();
            state.subscribers.retain(|sub| {
                eprintln!("Gotta do notifications for {:?}", event);
                match sub.try_send(event.clone()) {
                    Ok(()) => true,
                    Err(e) => match e {
                        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                            // Remove the sender from the subscriptions since they hung up on us (rude)
                            trace!(target: "rustydht_lib::DHT", "Removing channel for closed DHTEvent subscriber");
                            false
                        }
                        tokio::sync::mpsc::error::TrySendError::Full(_) => {
                            warn!(target: "rustydht_lib::DHT", "DHTEvent subscriber channel is full - can't send event {:?}", event);
                            true
                        }
                    }
                }
            });
        }

        return Ok(());
    }

    /// Runs the main event loop of the DHT.
    pub async fn run_event_loop(&self) -> Result<(), RustyDHTError> {
        match tokio::try_join!(
            // One-time
            self.ping_routers(self.shutdown.clone()),
            // Loop indefinitely
            self.accept_incoming_packets(),
            self.periodic_router_ping(self.shutdown.clone()),
            self.periodic_buddy_ping(self.shutdown.clone()),
            self.periodic_find_node(self.shutdown.clone()),
            self.periodic_ip4_maintenance(),
            self.periodic_token_rotation(),
            async {
                let to_ret: Result<(), RustyDHTError> = Err(RustyDHTError::ShutdownError(anyhow!(
                    "run_event_loop should shutdown"
                )));
                self.shutdown.clone().watch().await;
                return to_ret;
            }
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                if let RustyDHTError::ShutdownError(_) = e {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Subscribe to DHTEvent notifications from the DHT.
    pub fn subscribe(&self) -> mpsc::Receiver<DHTEvent> {
        let (tx, rx) = mpsc::channel(32);
        let mut state = self.state.lock().unwrap();
        state.subscribers.push(tx);
        rx
    }

    pub fn get_id(&self) -> Id {
        self.state.try_lock().unwrap().our_id
    }

    /// Returns tuple of (unverified, verified) nodes
    pub fn get_num_nodes(&self) -> (usize, usize) {
        self.state.try_lock().unwrap().buckets.count()
    }

    pub fn get_info_hashes(&self, newer_than: Option<Instant>) -> Vec<(Id, Vec<PeerInfo>)> {
        let state = self.state.try_lock().unwrap();
        let hashes = state.peer_storage.get_info_hashes();
        hashes
            .iter()
            .copied()
            .map(|hash| (hash, state.peer_storage.get_peers_info(&hash, newer_than)))
            .filter(|tup| tup.1.len() > 0)
            .collect()
    }

    async fn periodic_buddy_ping(
        &self,
        shutdown: shutdown::ShutdownReceiver,
    ) -> Result<(), RustyDHTError> {
        loop {
            let ping_check_interval_secs = self
                .state
                .try_lock()
                .unwrap()
                .settings
                .ping_check_interval_secs;
            sleep(Duration::from_secs(ping_check_interval_secs)).await;

            // Package things that need state into a block so that Rust will not complain about MutexGuard kept past .await
            let reverify_interval_secs = {
                let mut state = self.state.try_lock().unwrap();
                let count = state.buckets.count();
                debug!(target: "rustydht_lib::DHT",
                    "Pruning node buckets. Storage has {} unverified, {} verified in {} buckets",
                    count.0,
                    count.1,
                    state.buckets.count_buckets()
                );
                let reverify_grace_period_secs = state.settings.reverify_grace_period_secs;
                let verify_grace_period_secs = state.settings.verify_grace_period_secs;
                state.buckets.prune(
                    Duration::from_secs(reverify_grace_period_secs),
                    Duration::from_secs(verify_grace_period_secs),
                );

                state.settings.reverify_interval_secs
            };
            match Instant::now().checked_sub(Duration::from_secs(reverify_interval_secs)) {
                None => {
                    debug!(target: "rustydht_lib::DHT", "Monotonic clock underflow - skipping this round of pings");
                }

                Some(ping_if_older_than) => {
                    debug!(target: "rustydht_lib::DHT", "Sending pings to all nodes that have never verified or haven't been verified in a while");
                    let (unverified, verified) = {
                        let state = self.state.lock().unwrap();
                        (
                            state.buckets.get_all_unverified(),
                            state.buckets.get_all_verified(),
                        )
                    };
                    // Ping everybody we haven't verified
                    for wrapper in unverified {
                        // Some things in here are actually verified... don't bother them too often
                        if let Some(last_verified) = wrapper.last_verified {
                            if last_verified >= ping_if_older_than {
                                continue;
                            }
                            trace!(target: "rustydht_lib::DHT", "Sending ping to reverify backup {:?}", wrapper.node);
                        } else {
                            trace!(target: "rustydht_lib::DHT",
                                "Sending ping to verify {:?} (last seen {} seconds ago)",
                                wrapper.node,
                                (Instant::now() - wrapper.last_seen).as_secs()
                            );
                        }
                        let shutdown_clone = shutdown.clone();
                        self.ping_internal(
                            shutdown_clone,
                            wrapper.node.address,
                            Some(wrapper.node.id),
                        )
                        .await?;
                    }

                    // Reverify those who haven't been verified recently
                    for wrapper in verified {
                        if let Some(last_verified) = wrapper.last_verified {
                            if last_verified >= ping_if_older_than {
                                continue;
                            }
                        }
                        let shutdown_clone = shutdown.clone();
                        trace!(target: "rustydht_lib::DHT", "Sending ping to reverify {:?}", wrapper.node);
                        self.ping_internal(
                            shutdown_clone,
                            wrapper.node.address,
                            Some(wrapper.node.id),
                        )
                        .await?;
                    }
                }
            }
        }
    }

    async fn periodic_find_node(
        &self,
        shutdown: shutdown::ShutdownReceiver,
    ) -> Result<(), RustyDHTError> {
        loop {
            let find_node_interval_secs = self
                .state
                .try_lock()
                .unwrap()
                .settings
                .find_nodes_interval_secs;
            sleep(Duration::from_secs(find_node_interval_secs)).await;

            let (count_unverified, count_verified) = self.state.try_lock().unwrap().buckets.count();

            // If we don't know anybody, force a router ping.
            // This is helpful if we've been asleep for a while and lost all peers
            if count_verified <= 0 {
                let shutdown_clone = shutdown.clone();
                self.ping_routers(shutdown_clone).await?;
            }

            // Package things that need state into this block to avoid issues with MutexGuard kept over .await
            let (nearest_nodes, id_near_us) = {
                let state = self.state.try_lock().unwrap();
                if count_unverified > state.settings.find_nodes_skip_count {
                    debug!(target: "rustydht_lib::DHT", "Skipping find_node as we already have enough unverified");
                    continue;
                }

                let id_near_us = state.our_id.make_mutant();

                // Find the closest nodes to ask
                (
                    state.buckets.get_nearest_nodes(&id_near_us, None),
                    id_near_us,
                )
            };
            trace!(
                target: "rustydht_lib::DHT",
                "Sending find_node to {} nodes about {:?}",
                nearest_nodes.len(),
                id_near_us
            );
            for node in nearest_nodes {
                self.find_node_internal(shutdown.clone(), node.address, Some(node.id), id_near_us)
                    .await?;
            }
        }
    }

    async fn periodic_ip4_maintenance(&self) -> Result<(), RustyDHTError> {
        loop {
            sleep(Duration::from_secs(10)).await;

            let mut state = self.state.try_lock().unwrap();
            state.ip4_source.decay();

            if let Some(ip) = state.ip4_source.get_best_ipv4() {
                let ip = IpAddr::V4(ip);
                if !state.our_id.is_valid_for_ip(&ip) {
                    let new_id = Id::from_ip(&ip);
                    info!(target: "rustydht_lib::DHT",
                        "Our current id {} is not valid for IP {}. Using new id {}",
                        state.our_id,
                        ip,
                        new_id
                    );
                    state.our_id = new_id;
                    state.buckets.set_id(new_id);
                }
            }
        }
    }

    async fn periodic_router_ping(
        &self,
        shutdown: shutdown::ShutdownReceiver,
    ) -> Result<(), RustyDHTError> {
        loop {
            let router_ping_interval_secs = self
                .state
                .try_lock()
                .unwrap()
                .settings
                .router_ping_interval_secs;
            sleep(Duration::from_secs(router_ping_interval_secs)).await;
            debug!(target: "rustydht_lib::DHT", "Pinging routers");
            let shutdown_clone = shutdown.clone();
            self.ping_routers(shutdown_clone).await?;
        }
    }

    async fn periodic_token_rotation(&self) -> Result<(), RustyDHTError> {
        loop {
            sleep(Duration::from_secs(300)).await;
            self.rotate_token_secrets();
        }
    }

    /// Helper function for internal use. Spawns a new task and does a ping from there, with timeout.
    async fn ping_internal(
        &self,
        shutdown: shutdown::ShutdownReceiver,
        target: SocketAddr,
        target_id: Option<Id>,
    ) -> Result<(), RustyDHTError> {
        let state = self.state.clone();
        let socket = self.socket.clone();
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown,
            DHT::ping_impl(state, socket, target, target_id),
            format!("ping to {}", target),
            Some(Duration::from_secs(5)),
        );
        Ok(())
    }

    pub async fn ping(
        &self,
        target: SocketAddr,
        target_id: Option<Id>,
    ) -> Result<packets::Message, RustyDHTError> {
        let state = self.state.clone();
        let socket = self.socket.clone();
        DHT::ping_impl(state, socket, target, target_id).await
    }

    async fn ping_impl(
        state: Arc<Mutex<DHTState>>,
        socket: Arc<DHTSocket>,
        target: SocketAddr,
        target_id: Option<Id>,
    ) -> Result<packets::Message, RustyDHTError> {
        let our_id = {
            let state = state.try_lock().unwrap();
            state.our_id
        };
        let req = packets::Message::create_ping_request(our_id);
        let mut reply_channel = socket
            .send_to(req, target, target_id)
            .await?
            .expect("Didn't receive reply notification channel");

        match reply_channel.recv().await {
            Some(reply) => {
                match &reply.message_type {
                    packets::MessageType::Response(response_variant) => {
                        match response_variant {
                            packets::ResponseSpecific::PingResponse(arguments) => {
                                let is_id_valid =
                                    arguments.responder_id.is_valid_for_ip(&target.ip());
                                if !is_id_valid {
                                    return Ok(reply);
                                }

                                let mut state = state.try_lock().unwrap();
                                // If so, we'll take their vote on our IPv4 address and mark them as verified
                                DHT::ip4_vote_helper(&mut state, &target, &reply);
                                state
                                    .buckets
                                    .add_or_update(Node::new(arguments.responder_id, target), true);
                                Ok(reply)
                            }
                            _ => Err(RustyDHTError::GeneralError(anyhow!(
                                "Invalid response to ping"
                            ))),
                        }
                    }
                    _ => Err(RustyDHTError::GeneralError(anyhow!(
                        "Invalid response to ping"
                    ))),
                }
            }
            None => Err(RustyDHTError::GeneralError(anyhow!("sender hung up!?"))),
        }
    }

    async fn ping_router<G: AsRef<str>>(
        &self,
        shutdown: shutdown::ShutdownReceiver,
        hostname: G,
    ) -> Result<(), RustyDHTError> {
        let hostname = hostname.as_ref();
        // Resolve and add to request storage
        let resolve = lookup_host(hostname).await;
        if let Err(err) = resolve {
            // Used to only eat the specific errors corresponding to a failure to resolve,
            // but they vary by platform and it's a pain. For now, we'll eat all host
            // resolution errors.
            warn!(
                target: "rustydht_lib::DHT",
                "Failed to resolve host {} due to error {:#?}. Try again later.",
                hostname, err
            );
            return Ok(());
        }

        for socket_addr in resolve.unwrap() {
            if socket_addr.is_ipv4() {
                let shutdown_clone = shutdown.clone();
                self.ping_internal(shutdown_clone, socket_addr, None)
                    .await?;
                break;
            }
        }
        Ok(())
    }

    /// Pings some bittorrent routers
    async fn ping_routers(
        &self,
        shutdown: shutdown::ShutdownReceiver,
    ) -> Result<(), RustyDHTError> {
        let mut futures = futures::stream::FuturesUnordered::new();
        let routers = self.state.try_lock().unwrap().routers.clone();
        for hostname in routers {
            let shutdown_clone = shutdown.clone();
            futures.push(self.ping_router(shutdown_clone, hostname));
        }
        while let Some(result) = futures.next().await {
            result?;
        }
        Ok(())
    }

    fn rotate_token_secrets(&self) {
        let mut state = self.state.try_lock().unwrap();
        let new_token_secret = make_token_secret(state.settings.token_secret_size);

        state.old_token_secret = state.token_secret.clone();
        state.token_secret = new_token_secret;
        debug!(
            target: "rustydht_lib::DHT",
            "Rotating token secret. New secret is {:?}, old secret is {:?}",
            state.token_secret,
            state.old_token_secret
        );
    }

    async fn find_node_internal(
        &self,
        shutdown: shutdown::ShutdownReceiver,
        dest: SocketAddr,
        dest_id: Option<Id>,
        target: Id,
    ) -> Result<(), RustyDHTError> {
        let state = self.state.clone();
        let socket = self.socket.clone();
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown,
            DHT::find_node_impl(state, socket, dest, dest_id, target),
            format!("find_node to {} for {}", dest, target),
            Some(Duration::from_secs(5)),
        );
        Ok(())
    }

    pub async fn find_node(
        &self,
        dest: SocketAddr,
        dest_id: Option<Id>,
        target: Id,
    ) -> Result<packets::Message, RustyDHTError> {
        let state = self.state.clone();
        let socket = self.socket.clone();
        DHT::find_node_impl(state, socket, dest, dest_id, target).await
    }

    async fn find_node_impl(
        state: Arc<Mutex<DHTState>>,
        socket: Arc<DHTSocket>,
        dest: SocketAddr,
        dest_id: Option<Id>,
        target: Id,
    ) -> Result<packets::Message, RustyDHTError> {
        let our_id = state.try_lock().unwrap().our_id;
        let req = packets::Message::create_find_node_request(our_id, target);
        let mut reply_channel = socket
            .send_to(req, dest, dest_id)
            .await?
            .expect("Didn't receive reply notification channel");

        match reply_channel.recv().await {
            Some(reply) => {
                if let packets::MessageType::Response(
                    packets::ResponseSpecific::FindNodeResponse(arguments),
                ) = &reply.message_type
                {
                    let mut state = state.try_lock().unwrap();
                    DHT::ip4_vote_helper(&mut state, &dest, &reply);
                    state
                        .buckets
                        .add_or_update(Node::new(arguments.responder_id, dest), true);

                    // Add the nodes we got back as "seen" (even though we haven't necessarily seen them directly yet).
                    // They will be pinged later in an attempt to verify them.
                    for node in &arguments.nodes {
                        if node.id.is_valid_for_ip(&node.address.ip()) {
                            state.buckets.add_or_update(node.clone(), false);
                        }
                    }
                    Ok(reply)
                } else {
                    Err(RustyDHTError::GeneralError(anyhow!(
                        "Invalid response to find_node"
                    )))
                }
            }
            None => Err(RustyDHTError::GeneralError(anyhow!("sender hung up!?"))),
        }
    }

    /// Adds a 'vote' for whatever IP address the sender says we have.
    fn ip4_vote_helper(state: &mut DHTState, addr: &SocketAddr, msg: &packets::Message) {
        if let IpAddr::V4(their_ip) = addr.ip() {
            if let Some(they_claim_our_sockaddr) = &msg.requester_ip {
                if let SocketAddr::V4(they_claim_our_sockaddr) = they_claim_our_sockaddr {
                    state
                        .ip4_source
                        .add_vote(their_ip, they_claim_our_sockaddr.ip().clone());
                }
            }
        }
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

fn make_token_secret(size: usize) -> Vec<u8> {
    let mut token_secret = vec![0; size];
    token_secret.fill_with(|| thread_rng().gen());
    token_secret
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::common::ipv4_addr_src::StaticIPV4AddrSource;
    use anyhow::anyhow;
    use std::boxed::Box;

    async fn make_test_dht(
        port: u16,
    ) -> (DHT, shutdown::ShutdownSender, shutdown::ShutdownReceiver) {
        let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
        let phony_ip4 = Box::new(StaticIPV4AddrSource::new(ipv4));
        let buckets = |id| -> Box<dyn NodeStorage + Send> {
            Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                id, 8,
            ))
        };
        let (tx, rx) = shutdown::create_shutdown();
        (
            DHT::new(
                rx.clone(),
                Some(get_dht_id()),
                port,
                phony_ip4,
                buckets,
                &[],
                DHTSettings::default(),
            )
            .await
            .unwrap(),
            tx,
            rx,
        )
    }

    #[tokio::test]
    async fn test_responds_to_ping() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let ping_request = packets::Message::create_ping_request(requester_id);

        let port = 1948;
        let (dht, mut shutdown_tx, shutdown_rx) = make_test_dht(port).await;
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move {
                dht.run_event_loop().await.unwrap();
            },
            "Test DHT",
            Some(Duration::from_secs(10)),
        );

        let res = send_and_receive(ping_request.clone(), port).await.unwrap();

        assert_eq!(res.transaction_id, ping_request.transaction_id);
        assert_eq!(
            res.message_type,
            packets::MessageType::Response(packets::ResponseSpecific::PingResponse(
                packets::PingResponseArguments {
                    responder_id: get_dht_id()
                }
            ))
        );

        shutdown_tx.shutdown().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_responds_to_get_peers() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let desired_info_hash = Id::from_random(&mut thread_rng());
        let request = packets::Message::create_get_peers_request(requester_id, desired_info_hash);

        let port = 1974;
        let (dht, mut shutdown_tx, shutdown_rx) = make_test_dht(port).await;
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move {
                dht.run_event_loop().await.unwrap();
            },
            "Test DHT",
            Some(Duration::from_secs(10)),
        );

        let res = send_and_receive(request.clone(), port).await.unwrap();

        assert_eq!(res.transaction_id, request.transaction_id);
        assert!(matches!(
            res.message_type,
            packets::MessageType::Response(packets::ResponseSpecific::GetPeersResponse(
                packets::GetPeersResponseArguments { .. }
            ))
        ));

        shutdown_tx.shutdown().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_responds_to_find_node() -> Result<(), RustyDHTError> {
        let port = 1995;
        let (dht, mut shutdown_tx, shutdown_rx) = make_test_dht(port).await;
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move {
                dht.run_event_loop().await.unwrap();
            },
            "Test DHT",
            Some(Duration::from_secs(10)),
        );

        let requester_id = Id::from_random(&mut thread_rng());
        let target = Id::from_random(&mut thread_rng());
        let request = packets::Message::create_find_node_request(requester_id, target);
        let res = send_and_receive(request.clone(), port).await.unwrap();

        assert_eq!(res.transaction_id, request.transaction_id);
        assert!(matches!(
            res.message_type,
            packets::MessageType::Response(packets::ResponseSpecific::FindNodeResponse(
                packets::FindNodeResponseArguments { .. }
            ))
        ));

        shutdown_tx.shutdown().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_responds_to_announce_peer() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let info_hash = Id::from_random(&mut thread_rng());
        let port = 2014;
        let (dht, mut shutdown_tx, shutdown_rx) = make_test_dht(port).await;
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move {
                dht.run_event_loop().await.unwrap();
            },
            "Test DHT",
            Some(Duration::from_secs(10)),
        );

        // Send a get_peers request and get the response
        let reply = send_and_receive(
            packets::Message::create_get_peers_request(requester_id, info_hash),
            port,
        )
        .await
        .unwrap();

        // Extract the token from the get_peers response
        let token = {
            if let packets::MessageType::Response(packets::ResponseSpecific::GetPeersResponse(
                packets::GetPeersResponseArguments { token, .. },
            )) = reply.message_type
            {
                token
            } else {
                return Err(RustyDHTError::GeneralError(anyhow!("Didn't get token")));
            }
        };

        // Send an announce_peer request and get the response
        let reply = send_and_receive(
            packets::Message::create_announce_peer_request(
                requester_id,
                info_hash,
                1234,
                false,
                token,
            ),
            port,
        )
        .await
        .unwrap();

        // The response must be a ping response
        assert!(matches!(
            reply.message_type,
            packets::MessageType::Response(packets::ResponseSpecific::PingResponse(
                packets::PingResponseArguments { .. }
            ))
        ));

        // Send get peers again - this time we'll get a peer back (ourselves)
        let reply = send_and_receive(
            packets::Message::create_get_peers_request(requester_id, info_hash),
            port,
        )
        .await
        .unwrap();

        eprintln!("Received {:?}", reply);

        // Make sure we got a peer back
        let peers = {
            if let packets::MessageType::Response(packets::ResponseSpecific::GetPeersResponse(
                packets::GetPeersResponseArguments {
                    values: packets::GetPeersResponseValues::Peers(p),
                    ..
                },
            )) = reply.message_type
            {
                p
            } else {
                return Err(RustyDHTError::GeneralError(anyhow!("Didn't get peers")));
            }
        };
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].port(), 1234);
        eprintln!("all good!");
        shutdown_tx.shutdown().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_responds_to_sample_infohashes() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let target = Id::from_random(&mut thread_rng());
        let request = packets::Message::create_sample_infohashes_request(requester_id, target);

        let port = 2037;
        let (dht, mut shutdown_tx, shutdown_rx) = make_test_dht(port).await;
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move {
                dht.run_event_loop().await.unwrap();
            },
            "Test DHT",
            Some(Duration::from_secs(10)),
        );

        let res = send_and_receive(request.clone(), port).await.unwrap();

        assert_eq!(res.transaction_id, request.transaction_id);
        assert!(matches!(
            res.message_type,
            packets::MessageType::Response(packets::ResponseSpecific::SampleInfoHashesResponse(
                packets::SampleInfoHashesResponseArguments { num: 0, .. }
            ))
        ));

        shutdown_tx.shutdown().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_event_loop_pings_routers() {
        let (mut shutdown_tx, shutdown_rx) = shutdown::create_shutdown();
        let port1 = 2171;
        let dht1 = Arc::new(
            DHT::new(
                shutdown_rx.clone(),
                Some(get_dht_id()),
                port1,
                Box::new(StaticIPV4AddrSource::new(Ipv4Addr::new(1, 2, 3, 4))),
                |id| -> Box<dyn NodeStorage + Send> {
                    Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                        id, 8,
                    ))
                },
                &[],
                DHTSettings::default(),
            )
            .await
            .unwrap(),
        );

        let port2 = 2186;
        let settings2 = {
            let mut s = DHTSettings::default();
            s.router_ping_interval_secs = 1;
            s
        };
        let dht2 = Arc::new(
            DHT::new(
                shutdown_rx.clone(),
                Some(get_dht_id().make_mutant()),
                port2,
                Box::new(StaticIPV4AddrSource::new(Ipv4Addr::new(1, 2, 3, 4))),
                |id| -> Box<dyn NodeStorage + Send> {
                    Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                        id, 8,
                    ))
                },
                &[&format!("127.0.0.1:{}", port1)],
                settings2,
            )
            .await
            .unwrap(),
        );

        let mut receiver = dht2.subscribe();

        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx.clone(),
            async move {
                dht1.run_event_loop().await.unwrap();
            },
            "DHT1",
            None,
        );

        let dht2_clone = dht2.clone();
        shutdown::ShutdownReceiver::spawn_with_shutdown(
            shutdown_rx,
            async move { dht2_clone.run_event_loop().await.unwrap() },
            "DHT2",
            None,
        );

        receiver.recv().await;
        let (unverified, verified) = dht2.state.try_lock().unwrap().buckets.count();

        // Must drop dht2 as it contains a ShutdownReceiver channel which will block shutdown
        drop(dht2);

        shutdown_tx.shutdown().await;
        assert_eq!(unverified, 0);
        assert_eq!(verified, 1);
    }

    #[tokio::test]
    async fn test_token_secret_rotation() {
        let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
        let phony_ip4 = Box::new(StaticIPV4AddrSource::new(ipv4));
        let buckets = |id| -> Box<dyn NodeStorage + Send> {
            Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                id, 8,
            ))
        };
        let port = 2244;
        let dht = DHT::new(
            shutdown::create_shutdown().1,
            Some(get_dht_id()),
            port,
            phony_ip4,
            buckets,
            &[],
            DHTSettings::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            dht.state.try_lock().unwrap().token_secret.len(),
            DHTSettings::default().token_secret_size
        );

        dht.rotate_token_secrets();
        assert_eq!(
            dht.state.try_lock().unwrap().old_token_secret.len(),
            DHTSettings::default().token_secret_size
        );
        assert_eq!(
            dht.state.try_lock().unwrap().token_secret.len(),
            DHTSettings::default().token_secret_size
        );

        let state = dht.state.try_lock().unwrap();
        assert_ne!(state.old_token_secret, state.token_secret);
    }

    // Dumb helper function because we can't declare a const or static Id
    fn get_dht_id() -> Id {
        Id::from_hex("0011223344556677889900112233445566778899").unwrap()
    }

    // Helper function that sends a single packet to the test DHT and then returns the response
    async fn send_and_receive(
        msg: packets::Message,
        port: u16,
    ) -> Result<packets::Message, RustyDHTError> {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.send_to(
            &msg.clone().to_bytes().unwrap(),
            format!("127.0.0.1:{}", port),
        )
        .await
        .unwrap();
        let mut recv_buf = [0; 2048];
        let num_read = sock.recv_from(&mut recv_buf).await.unwrap().0;
        packets::Message::from_bytes(&recv_buf[..num_read])
    }
}

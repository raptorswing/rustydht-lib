use rand::prelude::SliceRandom;
use rand::{thread_rng, Rng};

use smol::lock::Mutex;
use smol::net::{resolve, UdpSocket};
use smol::{prelude::*, Timer};

use log::{debug, info, trace, warn};

extern crate crc;
use crc::{crc32, Hasher32};

use std::cell::RefCell;
use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant};

use crate::common::ipv4_addr_src::IPV4AddrSource;
use crate::common::{Id, Node};
use crate::errors::RustyDHTError;
use crate::packets;
use crate::storage::node_bucket_storage::NodeStorage;
use crate::storage::outbound_request_storage::{OutboundRequestStorage, RequestInfo};
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
                        info!(target: "DHT",
                            "Our external IPv4 is {:?}. Generated id {} based on that",
                            ip, id
                        );
                        id
                    }

                    None => {
                        let id = Id::from_random(&mut thread_rng());
                        info!(target: "DHT", "No external IPv4 provided. Using random id {} for now.", id);
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

        let token_secret = make_token_secret(settings.token_secret_size);

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
                        warn!(target: "DHT", "{:?}", internal);
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

        // Drop the packet if the IP has been throttled.
        if throttler.check_throttle(addr.ip(), None) {
            return Ok(());
        }

        // Filter out packets sent from port 0. We can't reply to these.
        if addr.port() == 0 {
            warn!(target: "DHT", "{} has invalid port - dropping packet", addr);
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

                    packets::RequestSpecific::SampleInfoHashesRequest(arguments) => {
                        let is_id_valid = arguments.requester_id.is_valid_for_ip(&addr.ip());
                        let mut buckets = self.buckets.lock().await;
                        if is_id_valid {
                            buckets.add_or_update(Node::new(arguments.requester_id, addr), false);
                        }

                        let nearest = buckets
                            .get_nearest_nodes(&arguments.target, Some(&arguments.requester_id))
                            .iter()
                            .map(|&n_ref| n_ref.clone())
                            .collect();

                        let (info_hashes, total_info_hashes) = {
                            let peer_storage = self.peer_storage.lock().await;
                            let info_hashes = peer_storage.get_info_hashes();
                            let total_info_hashes = info_hashes.len();
                            let info_hashes = {
                                let mut rng = thread_rng();
                                peer_storage
                                    .get_info_hashes()
                                    .as_mut_slice()
                                    .partial_shuffle(&mut rng, self.settings.max_sample_response)
                                    .0
                                    .to_vec()
                            };
                            (info_hashes, total_info_hashes)
                        };

                        let reply = packets::Message::create_sample_infohashes_response(
                            *self.our_id.borrow(),
                            msg.transaction_id,
                            addr,
                            Duration::from_secs(
                                self.settings.min_sample_interval_secs.try_into().unwrap(),
                            ),
                            nearest,
                            info_hashes,
                            total_info_hashes,
                        );
                        let reply_bytes = reply.to_bytes()?;
                        self.send_to(&reply_bytes, addr).await?;
                    }
                }
            }

            packets::MessageType::Response(response_variant) => {
                match response_variant {
                    packets::ResponseSpecific::PingResponse(arguments) => {
                        let is_id_valid = arguments.responder_id.is_valid_for_ip(&addr.ip());
                        if !is_id_valid {
                            return Ok(());
                        }
                        // Does this response correspond to a request we sent recently?
                        let has_matching_request = {
                            let mut request_storage = self.request_storage.lock().await;
                            request_storage.take_matching_request_info(&msg).is_some()
                        };

                        // If so, we'll take their vote on our IPv4 address and mark them as verified
                        if has_matching_request {
                            self.ip4_vote_helper(&addr, &msg).await;
                            let mut buckets = self.buckets.lock().await;
                            buckets.add_or_update(Node::new(arguments.responder_id, addr), true);
                        }
                    }

                    packets::ResponseSpecific::FindNodeResponse(arguments) => {
                        // Does this response correspond to a request we sent recently?
                        let has_matching_request = {
                            let mut request_storage = self.request_storage.lock().await;
                            request_storage.take_matching_request_info(&msg).is_some()
                        };

                        // If so, we'll take their vote on our IPv4 address, mark them as verified, and add the nodes they sent
                        if has_matching_request {
                            self.ip4_vote_helper(&addr, &msg).await;

                            let mut buckets = self.buckets.lock().await;
                            buckets.add_or_update(Node::new(arguments.responder_id, addr), true);

                            // Add the nodes we got back as "seen" (even though we haven't necessarily seen them directly yet).
                            // They will be pinged later in an attempt to verify them.
                            for node in &arguments.nodes {
                                if node.id.is_valid_for_ip(&node.address.ip()) {
                                    buckets.add_or_update(node.clone(), false);
                                }
                            }
                        } else {
                            debug!(target: "DHT", "Ignoring unsolicited find_node response");
                        }
                    }
                    _ => {
                        info!(target: "DHT",
                            "Received unsupported/unexpected KRPCResponse variant from {:?}: {:?}",
                            addr, response_variant
                        );
                    }
                }
            }
            _ => {
                warn!(target: "DHT",
                    "Received unsupported/unexpected KRPCMessage variant from {:?}: {:?}",
                    addr, msg
                );
            }
        }
        return Ok(());
    }

    /// Runs the main event loop of the DHT. Never returns!
    pub async fn run_event_loop(&self) -> Result<(), RustyDHTError> {
        if let Err(err) = futures::try_join!(
            // One-time
            self.ping_routers(),
            // Loop indefinitely
            self.accept_incoming_packets(),
            self.periodic_router_ping(),
            self.periodic_buddy_ping(),
            self.periodic_request_prune(),
            self.periodic_find_node(),
            self.periodic_ip4_maintenance(),
            self.periodic_token_rotation(),
        ) {
            return Err(err);
        }

        Ok(())
    }

    pub fn get_id(&self) -> Id {
        self.our_id.borrow().clone()
    }

    async fn periodic_buddy_ping(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(self.settings.ping_check_interval_secs)).await;
            let mut buckets = self.buckets.lock().await;
            let count = buckets.count();
            debug!(target: "DHT",
                "Pruning node buckets. Storage has {} unverified, {} verified in {} buckets",
                count.0,
                count.1,
                buckets.count_buckets()
            );
            buckets.prune(
                Duration::from_secs(self.settings.reverify_grace_period_secs),
                Duration::from_secs(self.settings.verify_grace_period_secs),
            );

            match Instant::now()
                .checked_sub(Duration::from_secs(self.settings.reverify_interval_secs))
            {
                None => {
                    debug!(target: "DHT", "Monotonic clock underflow - skipping this round of pings");
                }

                Some(ping_if_older_than) => {
                    debug!(target: "DHT", "Sending pings to nodes");
                    // Ping everybody we haven't verified
                    for wrapper in buckets.get_all_unverified() {
                        // Some things in here are actually verified... don't bother them too often
                        if let Some(last_verified) = wrapper.last_verified {
                            if last_verified >= ping_if_older_than {
                                continue;
                            }
                            trace!(target: "DHT", "Sending ping to reverify backup {:?}", wrapper.node);
                        } else {
                            trace!(target: "DHT",
                                "Sending ping to verify {:?} (last seen {} seconds ago)",
                                wrapper.node,
                                (Instant::now() - wrapper.last_seen).as_secs()
                            );
                        }
                        self.ping(wrapper.node.address).await?;
                    }

                    // Reverify those who haven't been verified recently
                    for wrapper in buckets.get_all_verified() {
                        if let Some(last_verified) = wrapper.last_verified {
                            if last_verified >= ping_if_older_than {
                                continue;
                            }
                        }
                        trace!(target: "DHT", "Sending ping to reverify {:?}", wrapper.node);
                        self.ping(wrapper.node.address).await?;
                    }
                }
            }
        }
    }

    async fn periodic_find_node(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(self.settings.find_nodes_interval_secs)).await;

            {
                let buckets = self.buckets.lock().await;
                let (count_unverified, count_verified) = buckets.count();

                // If we don't know anybody, force a router ping.
                // This is helpful if we've been asleep for a while and lost all peers
                if count_verified <= 0 {
                    self.ping_routers().await?;
                }

                if count_unverified > self.settings.find_nodes_skip_count {
                    debug!(target: "DHT", "Skipping find_node as we already have enough unverified");
                    continue;
                }
            }

            // Search a random node to get diversity
            // let rando =  MainlineId::from_random();
            let near_us = self.our_id.borrow().make_mutant();

            // self.send_find_node(&rando).await?;
            self.send_find_node(near_us).await?;
        }
    }

    async fn periodic_ip4_maintenance(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(10)).await;

            let mut ip4_source = self.ip4_source.lock().await;
            ip4_source.decay();

            if let Some(ip) = ip4_source.get_best_ipv4() {
                let ip = IpAddr::V4(ip);
                if !self.our_id.borrow().is_valid_for_ip(&ip) {
                    let new_id = Id::from_ip(&ip);
                    info!(target: "DHT",
                        "Our current id {} is not valid for IP {}. Using new id {}",
                        self.our_id.borrow(),
                        ip,
                        new_id
                    );
                    self.our_id.replace(new_id);
                    self.buckets.lock().await.set_id(new_id);
                }
            }
        }
    }

    async fn periodic_request_prune(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(
                self.settings.outgoing_reqiest_check_interval_secs,
            ))
            .await;
            let mut request_storage = self.request_storage.lock().await;
            debug!(target: "DHT",
                "Time to prune request storage (size {})",
                request_storage.len()
            );
            request_storage.prune_older_than(Duration::from_secs(
                self.settings.outgoing_request_prune_secs,
            ));
        }
    }

    async fn periodic_router_ping(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(self.settings.router_ping_interval_secs)).await;
            debug!(target:"DHT", "Pinging routers");
            self.ping_routers().await?;
        }
    }

    async fn periodic_token_rotation(&self) -> Result<(), RustyDHTError> {
        loop {
            Timer::after(Duration::from_secs(300)).await;
            self.rotate_token_secrets();
        }
    }

    async fn ping(&self, target: SocketAddr) -> Result<(), RustyDHTError> {
        let req = packets::Message::create_ping_request(*self.our_id.borrow());
        let req_bytes = req.clone().to_bytes()?;
        self.request_storage
            .lock()
            .await
            .add_request(RequestInfo::new(target, None, req));
        self.send_to(&req_bytes, target).await?;
        Ok(())
    }

    async fn ping_router<G: AsRef<str>>(&self, hostname: G) -> Result<(), RustyDHTError> {
        let hostname = hostname.as_ref();
        // Resolve and add to request storage
        let resolve = resolve(hostname).await;
        if let Err(err) = resolve {
            // Used to only eat the specific errors corresponding to a failure to resolve,
            // but they vary by platform and it's a pain. For now, we'll eat all host
            // resolution errors.
            warn!(
                target: "DHT",
                "Failed to resolve host {} due to error {:#?}. Try again later.",
                hostname, err
            );
            return Ok(());
            /*
            if let Some(errno) = err.raw_os_error() {
                // For windows
                if errno == 11001 {
                    warn!(target: "DHT", "Failed to resolve host {}. Try again later.", hostname);
                    return Ok(());
                }
            }
            return Err(err.into());
            */
        }

        for socket_addr in resolve.unwrap() {
            if socket_addr.is_ipv4() {
                self.ping(socket_addr).await?;
                break;
            }
        }
        Ok(())
    }

    /// Pings some bittorrent routers
    async fn ping_routers(&self) -> Result<(), RustyDHTError> {
        let mut futures = futures::stream::FuturesUnordered::new();
        for hostname in &self.routers {
            futures.push(self.ping_router(hostname));
        }
        while let Some(result) = futures.next().await {
            result?;
        }
        Ok(())
    }

    fn rotate_token_secrets(&self) {
        let token_secret = make_token_secret(self.settings.token_secret_size);

        *self.old_token_secret.borrow_mut() = self.token_secret.take();
        *self.token_secret.borrow_mut() = token_secret;
        debug!(
            target: "DHT",
            "Rotating token secret. New secret is {:?}, old secret is {:?}",
            self.token_secret.borrow(),
            self.old_token_secret.borrow()
        );
    }

    async fn send_find_node(&self, target_id: Id) -> Result<(), RustyDHTError> {
        let buckets = self.buckets.lock().await;
        let mut request_storage = self.request_storage.lock().await;

        // Find the closest nodes to ask
        let nearest = buckets.get_nearest_nodes(&target_id, None);
        trace!(
            target: "DHT",
            "Sending find_node to {} nodes about {:?}",
            nearest.len(),
            target_id
        );
        for node in nearest {
            let req = packets::Message::create_find_node_request(*self.our_id.borrow(), target_id);
            let bytes = req.clone().to_bytes()?;

            let request_info = RequestInfo::new(node.address, Some(node.id), req);
            request_storage.add_request(request_info);
            self.send_to(&bytes, node.address).await?;
        }
        Ok(())
    }

    async fn send_to(&self, bytes: &Vec<u8>, dest: SocketAddr) -> Result<(), RustyDHTError> {
        self.socket
            .send_to(bytes, dest)
            .await
            .map_err(|err| RustyDHTError::GeneralError(err.into()))?;
        Ok(())
    }

    /// Adds a 'vote' for whatever IP address the sender says we have.
    async fn ip4_vote_helper(&self, addr: &SocketAddr, msg: &packets::Message) {
        if let IpAddr::V4(their_ip) = addr.ip() {
            if let Some(they_claim_our_sockaddr) = &msg.requester_ip {
                if let SocketAddr::V4(they_claim_our_sockaddr) = they_claim_our_sockaddr {
                    self.ip4_source
                        .lock()
                        .await
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

    #[test]
    fn test_responds_to_announce_peer() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let info_hash = Id::from_random(&mut thread_rng());
        smol::block_on(async {
            let res = futures::try_join!(
                accept_packets(2),
                get_token_announce_peer(requester_id, info_hash)
            )
            .map(|res| res.1)?;

            assert!(matches!(
                res.message_type,
                packets::MessageType::Response(packets::ResponseSpecific::PingResponse(
                    packets::PingResponseArguments { .. }
                ))
            ));

            Ok(())
        })
    }

    #[test]
    fn test_responds_to_sample_infohashes() -> Result<(), RustyDHTError> {
        let requester_id = Id::from_random(&mut thread_rng());
        let target = Id::from_random(&mut thread_rng());
        smol::block_on(async {
            let request = packets::Message::create_sample_infohashes_request(requester_id, target);

            let res = futures::try_join!(accept_single_packet(), send_and_receive(request.clone()))
                .map(|res| res.1)?;

            assert_eq!(res.transaction_id, request.transaction_id);
            assert!(matches!(
                res.message_type,
                packets::MessageType::Response(
                    packets::ResponseSpecific::SampleInfoHashesResponse(
                        packets::SampleInfoHashesResponseArguments { num: 0, .. }
                    )
                )
            ));

            Ok(())
        })
    }

    #[test]
    fn test_handles_ping_response() -> Result<(), RustyDHTError> {
        smol::block_on(async {
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

            let server_id = dht.our_id.borrow().clone();

            let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client_addr = client_socket.local_addr().unwrap();
            let client_id = Id::from_random(&mut thread_rng());
            let req = packets::Message::create_ping_request(server_id);
            {
                let mut request_storage = dht.request_storage.lock().await;
                request_storage.add_request(RequestInfo::new(client_addr, None, req.clone()));
            }

            let res = packets::Message::create_ping_response(
                client_id,
                req.transaction_id,
                "127.0.0.1:10001".parse().unwrap(),
            );

            let mut throttler = crate::storage::throttler::Throttler::new(
                100,
                Duration::from_secs(1),
                Duration::from_secs(1),
            );
            let mut recv_buf = [0; 2048];

            futures::try_join!(
                dht.accept_single_packet(&mut throttler, &mut recv_buf),
                send_only(res)
            )?;

            let num_verified = {
                let buckets = dht.buckets.lock().await;
                let verified = buckets.get_all_verified();
                verified.len()
            };
            assert_eq!(num_verified, 1);

            Ok(())
        })
    }

    #[test]
    fn test_handles_find_node_response() -> Result<(), RustyDHTError> {
        smol::block_on(async {
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

            let server_id = dht.our_id.borrow().clone();

            let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client_addr = client_socket.local_addr().unwrap();
            let client_id = Id::from_random(&mut thread_rng());
            let req = packets::Message::create_find_node_request(
                server_id,
                Id::from_random(&mut thread_rng()),
            );
            {
                let mut request_storage = dht.request_storage.lock().await;
                request_storage.add_request(RequestInfo::new(client_addr, None, req.clone()));
            }

            let returned_node_id = Id::from_random(&mut thread_rng());
            let res = packets::Message::create_find_node_response(
                client_id,
                req.transaction_id,
                "127.0.0.1:10001".parse().unwrap(),
                vec![Node::new(
                    returned_node_id,
                    "127.0.0.2:5050".parse().unwrap(),
                )],
            );

            let mut throttler = crate::storage::throttler::Throttler::new(
                100,
                Duration::from_secs(1),
                Duration::from_secs(1),
            );
            let mut recv_buf = [0; 2048];

            futures::try_join!(
                dht.accept_single_packet(&mut throttler, &mut recv_buf),
                send_only(res)
            )?;

            let buckets = dht.buckets.lock().await;
            let verified = buckets.get_all_verified();
            let unverified = buckets.get_all_unverified();
            assert_eq!(verified.len(), 1);
            assert_eq!(verified[0].node.id, client_id);
            assert_eq!(unverified.len(), 1);

            Ok(())
        })
    }

    #[test]
    fn test_event_loop_pings_routers() {
        smol::block_on(async {
            let router_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let router_addr = router_socket.local_addr().unwrap();
            let router_id = Id::from_random(&mut thread_rng());

            let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
            let phony_ip4 = Box::new(StaticIPV4AddrSource::new(ipv4));
            let buckets = |id| -> Box<dyn NodeStorage> {
                Box::new(crate::storage::node_bucket_storage::NodeBucketStorage::new(
                    id, 8,
                ))
            };
            let mut settings = DHTSettings::default();
            settings.router_ping_interval_secs = 1;
            let _lock = LOCK.lock();
            let dht = DHT::new(
                Some(get_dht_id()),
                10001,
                phony_ip4,
                buckets,
                &[&router_addr.to_string()],
                DHTSettings::default(),
            )
            .unwrap();

            smol::future::race(dht.run_event_loop(), async {
                let mut recv_buf = [0; 2048];
                let (num_read, remote) = router_socket.recv_from(&mut recv_buf).await.unwrap();
                let msg = packets::Message::from_bytes(&recv_buf[..num_read]).unwrap();
                assert!(matches!(
                    msg.message_type,
                    packets::MessageType::Request(packets::RequestSpecific::PingRequest(
                        packets::PingRequestArguments { .. }
                    ))
                ));

                let res =
                    packets::Message::create_ping_response(router_id, msg.transaction_id, remote);
                router_socket
                    .send_to(&res.to_bytes().unwrap(), remote)
                    .await
                    .expect("Failed to send_to");

                // Send our own ping and await the response - this is used instead of a hacky timer to know when the DHT has processed our response
                let req = packets::Message::create_ping_request(router_id);
                router_socket
                    .send_to(&req.to_bytes().unwrap(), remote)
                    .await
                    .expect("Failed to send_to");
                router_socket.recv_from(&mut recv_buf).await.unwrap();
                Ok(())
            })
            .await
            .expect("Got an error");

            let (unverified, verified) = dht.buckets.lock().await.count();
            assert_eq!(unverified, 0);
            assert_eq!(verified, 1);
        })
    }

    #[test]
    fn test_token_secret_rotation() {
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

        assert_eq!(
            dht.token_secret.borrow().len(),
            DHTSettings::default().token_secret_size
        );

        dht.rotate_token_secrets();
        assert_eq!(
            dht.old_token_secret.borrow().len(),
            DHTSettings::default().token_secret_size
        );
        assert_eq!(
            dht.token_secret.borrow().len(),
            DHTSettings::default().token_secret_size
        );
        assert_ne!(*dht.old_token_secret.borrow(), *dht.token_secret.borrow());
    }

    // Dumb helper function because we can't declare a const or static Id
    fn get_dht_id() -> Id {
        Id::from_hex("0011223344556677889900112233445566778899").unwrap()
    }

    // Helper function for test_announce_peer. When will async closure be stable!?
    async fn get_token_announce_peer(
        requester_id: Id,
        info_hash: Id,
    ) -> Result<packets::Message, RustyDHTError> {
        let req = packets::Message::create_get_peers_request(requester_id, info_hash);
        let res = send_and_receive(req).await?;

        if let packets::MessageType::Response(packets::ResponseSpecific::GetPeersResponse(
            packets::GetPeersResponseArguments { token, .. },
        )) = res.message_type
        {
            let req = packets::Message::create_announce_peer_request(
                requester_id,
                info_hash,
                1234,
                true,
                token,
            );
            send_and_receive(req).await
        } else {
            Err(RustyDHTError::GeneralError(anyhow!("Wrong packet")))
        }
    }

    // Helper function that creates a test DHT, handles a single packet, and returns
    async fn accept_single_packet() -> Result<(), RustyDHTError> {
        accept_packets(1).await
    }

    // Helper function creates test DHT, handles given number of packets, and returns
    async fn accept_packets(num: usize) -> Result<(), RustyDHTError> {
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

        for _ in 0..num {
            dht.accept_single_packet(&mut throttler, &mut recv_buf)
                .await?;
        }
        Ok(())
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

    async fn send_only(msg: packets::Message) -> Result<(), RustyDHTError> {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.send_to(&msg.clone().to_bytes().unwrap(), "127.0.0.1:10001")
            .await
            .unwrap();
        Ok(())
    }
}

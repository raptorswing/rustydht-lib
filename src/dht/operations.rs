use crate::common::{Id, Node};
use crate::dht::DHT;
use crate::errors::RustyDHTError;
use crate::packets;
use crate::packets::MessageBuilder;
use crate::storage::buckets::Buckets;
use crate::storage::node_wrapper::NodeWrapper;
use futures::StreamExt;
use log::{debug, error, info, trace, warn};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Announce that you are a peer for a specific info_hash, returning the nodes
/// that were successfully announced to.
///
/// # Arguments
/// * `dht` - DHT instance that will be used to communicate
/// * `info_hash` - Id of the torrent
/// * `port` - optional port that other peers should use to contact your peer.
/// If omitted, `implied_port` will be set true on the announce messages and
/// * `timeout` - the maximum amount of time that will be spent searching for
/// peers close to `info_hash` before announcing to them. This means that this
/// function can actually take a bit longer than `timeout`, since it will take
/// a moment after `timeout` has elapsed to announce to the nodes.
pub async fn announce_peer(
    dht: &DHT,
    info_hash: Id,
    port: Option<u16>,
    timeout: Duration,
) -> Vec<Node> {
    let mut to_ret = Vec::new();

    // Figure out which nodes we want to announce to
    let get_peers_result = get_peers(dht, info_hash, timeout).await;

    trace!(target:"rustydht_lib::operations::announce_peer", "{} nodes responded to get_peers", get_peers_result.responders.len());

    let announce_builder = MessageBuilder::new_announce_peer_request()
        .sender_id(dht.get_id())
        .read_only(dht.get_settings().read_only)
        .target(info_hash)
        .port(match port {
            Some(p) => p,
            None => 0,
        })
        .implied_port(match port {
            Some(_) => false,
            None => true,
        });

    // Prepare to send packets to the nearest 8
    let mut todos = futures::stream::FuturesUnordered::new();
    for responder in get_peers_result.responders.into_iter().take(8) {
        let builder = announce_builder.clone();
        todos.push(async move {
            let announce_req = builder
                .token(responder.token)
                .build()
                .expect("Failed to build announce_peer request");
            match dht
                .send_request(
                    announce_req,
                    responder.node.address,
                    Some(responder.node.id),
                    Some(Duration::from_secs(5)),
                )
                .await
            {
                Ok(_) => Ok(responder.node),
                Err(e) => Err(e),
            }
        });
    }

    // Execute the futures, handle their results
    while let Some(announce_result) = todos.next().await {
        match announce_result {
            Ok(node) => {
                to_ret.push(node);
            }

            Err(e) => match e {
                RustyDHTError::TimeoutError(_) => {
                    debug!(target: "rustydht_lib::operations::announce_peer", "announce_peer timed out: {}", e);
                }

                _ => {
                    warn!(target: "rustydht_lib::operations::announce_peer", "Error sending announce_peer: {}", e);
                }
            },
        }
    }

    to_ret
}

/// Use the DHT to find the closest nodes to the target as possible.
///
/// This runs until it stops making progress or `timeout` has elapsed.
pub async fn find_node(dht: &DHT, target: Id, timeout: Duration) -> Vec<Node> {
    let mut buckets = Buckets::new(target, 8);
    let dht_settings = dht.get_settings();

    if let Err(_) = tokio::time::timeout(timeout, async {
        let mut best_ids = Vec::new();
        loop {
            // Seed our buckets with the main buckets from the DHT
            for node_wrapper in dht.get_nodes() {
                if !buckets.contains(&node_wrapper.node.id) {
                    buckets.add(node_wrapper, None);
                }
            }

            // Grab a few nodes closest to our target
            let nearest = buckets.get_nearest_nodes(&target, None);
            if nearest.len() <= 0 {
                // If there are no nodes in the buckets yet, DHT may still be bootstrapping. Give it a moment and try again
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            let best_ids_current: Vec<Id> = nearest.iter().map(|nw| nw.node.id.clone()).collect();
            if best_ids == best_ids_current {
                break;
            }
            best_ids = best_ids_current;

            // Get ready to send get_peers to all of those closest nodes
            let request_builder = MessageBuilder::new_find_node_request()
                .target(target)
                .read_only(dht_settings.read_only)
                .sender_id(dht.get_id());
            let mut todos = futures::stream::FuturesUnordered::new();
            for node in nearest {
                todos.push(dht.send_request(
                    request_builder
                        .clone()
                        .build()
                        .expect("Failed to build find_node request"),
                    node.node.address,
                    Some(node.node.id),
                    Some(Duration::from_secs(5))
                ));
            }

            // Send get_peers to nearest nodes, handle their responses
            let started_sending_time = Instant::now();
            while let Some(request_result) = todos.next().await {
                match request_result {
                    Ok(message) => match message.message_type {
                        packets::MessageType::Response(
                            packets::ResponseSpecific::FindNodeResponse(args),
                        ) => {
                            for node in args.nodes {
                                if !buckets.contains(&node.id) {
                                    trace!(target: "rustydht_lib::operations::find_node", "Node {:?} is a candidate for buckets", node);
                                    buckets.add(NodeWrapper::new(node), None);
                                }
                            }
                        }

                        _ => {
                            error!(target: "rustydht_lib::operations::find_node", "Got wrong packet type back: {:?}", message);
                        }
                    },
                    Err(e) => {
                        warn!(target: "rustydht_lib::operations::find_node", "Error sending find_node request: {}", e);
                    }
                }
            }

            // Ensure that our next round of packet sending starts at least 1s from the last
            // to prevent us from hitting other nodes too hard.
            // i.e. don't be a jerk.
            let since_sent = Instant::now().saturating_duration_since(started_sending_time);
            let desired_interval = Duration::from_millis(1000);
            let needed_sleep_interval = desired_interval.saturating_sub(since_sent);
            if needed_sleep_interval != Duration::ZERO {
                tokio::time::sleep(needed_sleep_interval).await;
            }
        }
    })
    .await
    {
        debug!(target: "rustydht_lib::operations::find_node", "Timed out after {:?}", timeout);
    }

    buckets
        .get_nearest_nodes(&target, None)
        .into_iter()
        .map(|nw| nw.node.clone())
        .collect()
}

/// Use the DHT to retrieve peers for the given info_hash.
///
/// Returns the all the results so far after at least `desired_peers`
/// peers have been found, or `timeout` has elapsed (whichever happens first)
pub async fn get_peers(dht: &DHT, info_hash: Id, timeout: Duration) -> GetPeersResult {
    let mut unique_peers = HashSet::new();
    let mut responders = Vec::new();
    let mut buckets = Buckets::new(info_hash, 8);
    let dht_settings = dht.get_settings();

    // Hack to aid in bootstrapping
    find_node(dht, info_hash, Duration::from_secs(5)).await;

    if let Err(_) = tokio::time::timeout(timeout,
    async {
        let mut best_ids = Vec::new();
        loop {
            // Populate our buckets with the main buckets from the DHT
            for node_wrapper in dht.get_nodes() {
                if !buckets.contains(&node_wrapper.node.id) {
                    buckets.add(node_wrapper, None);
                }
            }

            // Grab a few nodes closest to our target info_hash
            let nearest = buckets.get_nearest_nodes(&info_hash, None);
            if nearest.len() <= 5 {
                // If there are no/few nodes in the buckets yet, DHT may still be bootstrapping. Give it a moment and try again
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            let best_ids_current: Vec<Id> = nearest.iter().map(|nw| nw.node.id.clone()).collect();
            if best_ids == best_ids_current {
                break;
            }
            best_ids = best_ids_current;

            // Get ready to send get_peers to all of those closest nodes
            let request_builder = MessageBuilder::new_get_peers_request()
                .target(info_hash)
                .read_only(dht_settings.read_only)
                .sender_id(dht.get_id());
            let mut todos = futures::stream::FuturesUnordered::new();
            for node in nearest {
                let node_clone = node.clone();
                let request_builder_clone = request_builder.clone();
                todos.push(async move {
                    match dht.send_request(
                        request_builder_clone
                            .build()
                            .expect("Failed to build get_peers request"),
                        node_clone.node.address,
                        Some(node_clone.node.id),
                        Some(Duration::from_secs(5))
                    ).await {
                        Ok(reply) => Ok((node_clone.node, reply)),
                        Err(e) => Err(e)
                    }
                });
            }

            // Send get_peers to nearest nodes, handle their responses
            let started_sending_time = Instant::now();
            while let Some(request_result) = todos.next().await {
                match request_result {
                    Ok(result) => match result.1.message_type {
                        packets::MessageType::Response(
                            packets::ResponseSpecific::GetPeersResponse(args),
                        ) => {
                            responders.push(GetPeersResponder{
                                node: result.0,
                                token: args.token
                            });

                            match args.values {
                            packets::GetPeersResponseValues::Nodes(n) => {
                                debug!(target: "rustydht_lib::operations::get_peers", "Got {} nodes", n.len());
                                for node in n {
                                    if !buckets.contains(&node.id) {
                                        trace!(target: "rustydht_lib::operations::get_peers", "Node {:?} is a candidate for buckets", node);
                                        buckets.add(NodeWrapper::new(node), None);
                                    }
                                }
                            }
                            packets::GetPeersResponseValues::Peers(p) => {
                                info!(target: "rustydht_lib::operations::get_peers", "Got {} peers", p.len());
                                for peer in p {
                                    unique_peers.insert(peer);
                                }
                            }
                        }},
                        _ => {
                            error!(target: "rustydht_lib::operations::get_peers", "Got wrong packet type back: {:?}", result.1);
                        }
                    },
                    Err(e) => {
                        warn!(target: "rustydht_lib::operations::get_peers", "Error sending get_peers request: {}", e);
                    }
                }
            }

            // Ensure that our next round of packet sending starts at least 1s from the last
            // to prevent us from hitting other nodes too hard.
            // i.e. don't be a jerk.
            let since_sent = Instant::now().saturating_duration_since(started_sending_time);
            let desired_interval = Duration::from_millis(1000);
            let needed_sleep_interval = desired_interval.saturating_sub(since_sent);
            if needed_sleep_interval != Duration::ZERO {
                tokio::time::sleep(needed_sleep_interval).await;
            }
        }
    }).await {
        debug!(target: "rustydht_lib::operations::get_peers", "Timed out after {:?}, returning current results", timeout);
    }

    GetPeersResult::new(info_hash, unique_peers.into_iter().collect(), responders)
}

pub struct GetPeersResult {
    pub info_hash: Id,
    pub peers: Vec<SocketAddr>,
    pub responders: Vec<GetPeersResponder>,
}

impl GetPeersResult {
    pub fn new(
        info_hash: Id,
        peers: Vec<SocketAddr>,
        mut responders: Vec<GetPeersResponder>,
    ) -> GetPeersResult {
        responders.sort_unstable_by(|a, b| {
            let a_dist = a.node.id.xor(&info_hash);
            let b_dist = b.node.id.xor(&info_hash);
            a_dist.partial_cmp(&b_dist).unwrap()
        });
        GetPeersResult {
            info_hash: info_hash,
            peers: peers,
            responders: responders,
        }
    }
}

pub struct GetPeersResponder {
    pub node: Node,
    pub token: Vec<u8>,
}

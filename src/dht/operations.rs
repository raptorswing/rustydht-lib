use crate::common::{Id, Node};
use crate::dht::DHT;
use crate::packets;
use crate::packets::MessageBuilder;
use crate::storage::buckets::Buckets;
use crate::storage::node_wrapper::NodeWrapper;
use futures::StreamExt;
use log::{debug, error, info, trace, warn};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Use the DHT to find the closes nodes to the target as possible.
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
pub async fn get_peers(
    dht: &DHT,
    info_hash: Id,
    desired_peers: usize,
    timeout: Duration,
) -> Vec<SocketAddr> {
    let mut to_ret = HashSet::new();
    let mut buckets = Buckets::new(info_hash, 8);
    let dht_settings = dht.get_settings();

    if let Err(_) = tokio::time::timeout(timeout,
    async {
        while to_ret.len() < desired_peers {
            // Populate our buckets with the main buckets from the DHT
            for node_wrapper in dht.get_nodes() {
                if !buckets.contains(&node_wrapper.node.id) {
                    buckets.add(node_wrapper, None);
                }
            }

            // Grab a few nodes closest to our target info_hash
            let nearest = buckets.get_nearest_nodes(&info_hash, None);

            // Get ready to send get_peers to all of those closest nodes
            let request_builder = MessageBuilder::new_get_peers_request()
                .target(info_hash)
                .read_only(dht_settings.read_only)
                .sender_id(dht.get_id());
            let mut todos = futures::stream::FuturesUnordered::new();
            for node in nearest {
                todos.push(dht.send_request(
                    request_builder
                        .clone()
                        .build()
                        .expect("Failed to build get_peers request"),
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
                            packets::ResponseSpecific::GetPeersResponse(args),
                        ) => match args.values {
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
                                    to_ret.insert(peer);
                                }
                            }
                        },
                        _ => {
                            error!(target: "rustydht_lib::operations::get_peers", "Got wrong packet type back: {:?}", message);
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
        debug!(target: "rustydht_lib::operations::get_peers", "Timed out after {:?}", timeout);
    }

    to_ret.into_iter().collect()
}

use crate::common::Id;
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

pub async fn get_peers(dht: &DHT, info_hash: Id, min_peers: usize) -> Vec<SocketAddr> {
    let mut to_ret = HashSet::new();
    let mut buckets = Buckets::new(info_hash, 8);
    let dht_settings = dht.get_settings();

    while to_ret.len() < min_peers {
        // Prepopulate our buckets with the main buckets from the DHT
        for node_wrapper in dht.get_nodes() {
            if let None = buckets.get_mut(&node_wrapper.node.id) {
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
            let fut = tokio::time::timeout(
                Duration::from_secs(10),
                dht.send_request(
                    request_builder
                        .clone()
                        .build()
                        .expect("Failed to build get_peers request"),
                    node.node.address,
                    Some(node.node.id),
                ),
            );
            todos.push(fut);
        }

        // Send get_peers to nearest nodes, handle their responses
        let started_sending_time = Instant::now();
        while let Some(timeout_result) = todos.next().await {
            match timeout_result {
                Ok(request_result) => match request_result {
                    Ok(message) => match message.message_type {
                        packets::MessageType::Response(
                            packets::ResponseSpecific::GetPeersResponse(args),
                        ) => match args.values {
                            packets::GetPeersResponseValues::Nodes(n) => {
                                debug!(target: "rustydht_lib::operations::get_peers", "Got {} nodes", n.len());
                                for node in n {
                                    if let None = buckets.get_mut(&node.id) {
                                        trace!(target: "rustydht_lib::operations::get_peers", "Adding node {:?} to buckets", node);
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
                },
                Err(e) => {
                    debug!(target: "rustydht_lib::operations::get_peers", "get_peers request timed out: {}", e);
                }
            }
        }

        // Ensure that our next round of packet sending starts at least 2s from the last
        // to prevent us from hitting other nodes too hard.
        // i.e. don't be a jerk.
        let since_sent = Instant::now().saturating_duration_since(started_sending_time);
        let desired_interval = Duration::from_millis(2000);
        let needed_sleep_interval = desired_interval.saturating_sub(since_sent);
        if needed_sleep_interval != Duration::ZERO {
            tokio::time::sleep(needed_sleep_interval).await;
        }
    }

    to_ret.into_iter().collect()
}

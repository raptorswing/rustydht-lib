#[macro_use]
extern crate log;

use clap::{App, Arg};
use log::LevelFilter;
use rustydht_lib::common::ipv4_addr_src::IPV4Consensus;
use rustydht_lib::dht;
use rustydht_lib::storage::node_bucket_storage::{NodeBucketStorage, NodeStorage};
use simple_logger::SimpleLogger;
use std::sync::Arc;
use warp::Filter;

const ROUTERS: [&str; 3] = [
    "router.bittorrent.com:6881",
    "router.utorrent.com:6881",
    "dht.transmissionbt.com:6881",
];

#[tokio::main(flavor = "current_thread")]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Error)
        .with_module_level("rustydht_lib::", LevelFilter::Info)
        .init()
        .expect("Failed to initialize logging");

    let matches = App::new("dht_node")
        .version("0.1")
        .author("raptorswing")
        .about("Example application for rustydht-lib. Acts as a Node on the mainline BitTorrent DHT network, runs a small HTTP status page.")
        .arg(
            Arg::with_name("listen_port")
                .short("l")
                .default_value("6881")
                .help("UDP port that the DHT should use to communicate with the network"))
        .arg(
            Arg::with_name("http_port")
                .short("h")
                .default_value("8080")
                .help("TCP port that the HTTP status server should listen on"))
        .arg(
            Arg::with_name("initial_id")
                .short("i")
                .required(false)
                .takes_value(true)
            .help("Id that the DHT node should start with. If not provided, a random one will be used."))
        .get_matches();

    let port: u16 = matches
        .value_of("listen_port")
        .expect("No value specified for listen port")
        .parse()
        .expect("Invalid value for listen port");
    let http_port: u16 = matches
        .value_of("http_port")
        .expect("No value specified for HTTP port")
        .parse()
        .expect("Invalid value for http port");

    let initial_id = match matches.value_of("initial_id") {
        None => None,
        Some(arg) => Some(
            rustydht_lib::common::Id::from_hex(&arg).expect("Failed to parse initial_id into Id"),
        ),
    };

    let (mut shutdown_tx, shutdown_rx) = rustydht_lib::shutdown::create_shutdown();
    let ip_source = Box::new(IPV4Consensus::new(2, 10));
    let buckets = |id| -> Box<dyn NodeStorage + Send> { Box::new(NodeBucketStorage::new(id, 8)) };
    let dht = Arc::new(
        dht::DHT::new(
            shutdown_rx.clone(),
            initial_id,
            port,
            ip_source,
            buckets,
            &ROUTERS,
            dht::DHTSettings::default(),
        )
        .await
        .expect("Failed to init DHT"),
    );

    let http_server = {
        let handler = {
            let dht = dht.clone();
            warp::path::end().map(move || {
                let id = dht.get_id();
                let nodes = dht.get_nodes();
                let nodes_string = {
                    let mut to_ret = String::new();
                    for node in &nodes {
                        to_ret += &format!("{}\t{}\n", node.node.id, node.node.address);
                    }
                    to_ret
                };
                let info_hashes_string = {
                    let info_hashes = dht.get_info_hashes(None);
                    let mut to_ret = String::new();
                    for hash in info_hashes {
                        to_ret += &format!("{}\t{}\n", hash.0, hash.1.len());
                    }
                    to_ret
                };

                format!(
                    "Id: {id}\n{nodes_count} nodes:\n{nodes_string}\nInfo hashes:\n{info_hashes_string}",
                    id=id, nodes_count=nodes.len(),nodes_string=nodes_string, info_hashes_string=info_hashes_string
                )
            })
        };

        let mut shutdown_rx = shutdown_rx.clone();
        let (_addr, server) = warp::serve(handler).bind_with_graceful_shutdown(
            ([127, 0, 0, 1], http_port),
            async move {
                shutdown_rx.watch().await;
            },
        );

        server
    };

    tokio::select! {
        _ = dht.run_event_loop() => {},
        _ = http_server => {},
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+c detected - sending shutdown signal");
            drop(dht);
            drop(shutdown_rx);
            shutdown_tx.shutdown().await;
        },
    };
}
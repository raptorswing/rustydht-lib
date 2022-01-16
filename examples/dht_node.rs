#[macro_use]
extern crate log;

use clap::{App, Arg};
use log::LevelFilter;
use rustydht_lib::dht;
use simple_logger::SimpleLogger;
use std::net::SocketAddr;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use warp::Filter;

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Error)
        .with_module_level("rustydht_lib::", LevelFilter::Info)
        .init()
        .expect("Failed to initialize logging");

    let matches = App::new("dht_node")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Example application for rustydht-lib. Acts as a Node on the mainline BitTorrent DHT network, and runs a small HTTP status page.")
        .arg(
            Arg::with_name("listen_port")
            .short("l")
            .default_value("6881")
            .help("UDP port that the DHT should use to communicate with the network"))
        .arg(
            Arg::with_name("http")
                .short("h")
                .default_value("127.0.0.1:8080")
                .help("Socket address that the HTTP status server will listen on for TCP connections"))
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
    let http_sockaddr: SocketAddr = matches
        .value_of("http")
        .expect("No value specified for 'http'")
        .parse()
        .expect("Invalid value for 'http'");

    let mut builder =
        dht::DHTBuilder::new().listen_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port));

    if let Some(arg) = matches.value_of("initial_id") {
        builder = builder.initial_id(
            rustydht_lib::common::Id::from_hex(&arg).expect("Failed to parse initial_id into Id"),
        );
    }

    let (mut shutdown_tx, shutdown_rx) = rustydht_lib::shutdown::create_shutdown();
    let dht = Arc::new(
        builder
            .build(shutdown_rx.clone())
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
        let (_addr, server) =
            warp::serve(handler).bind_with_graceful_shutdown(http_sockaddr, async move {
                shutdown_rx.watch().await;
            });

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

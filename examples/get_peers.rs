use clap::{App, Arg};
use log::LevelFilter;
use rustydht_lib::common::ipv4_addr_src::IPV4Consensus;
use rustydht_lib::common::Id;
use rustydht_lib::dht;
use rustydht_lib::dht::operations;
use rustydht_lib::shutdown;
use rustydht_lib::storage::node_bucket_storage::{NodeBucketStorage, NodeStorage};
use simple_logger::SimpleLogger;
use std::sync::Arc;

const ROUTERS: [&str; 3] = [
    "router.bittorrent.com:6881",
    "router.utorrent.com:6881",
    "dht.transmissionbt.com:6881",
];

#[tokio::main(flavor = "current_thread")]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .with_module_level("rustydht_lib::operations", LevelFilter::Trace)
        .init()
        .expect("Failed to initialize logging");

    let cmdline_matches = App::new("get_peers")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Example application for rustydht-lib. Retrieves peers for an info_hash.")
        .arg(
            Arg::with_name("dht_listen_port")
                .short("l")
                .default_value("6881"),
        )
        .arg(
            Arg::with_name("info_hash")
                .short("i")
                .required(true)
                .takes_value(true),
        )
        .get_matches();

    let port: u16 = cmdline_matches
        .value_of("dht_listen_port")
        .expect("No value specified for listen port")
        .parse()
        .expect("Invalid value for listen port");

    let info_hash = Id::from_hex(
        &cmdline_matches
            .value_of("info_hash")
            .expect("No value specified for info_hash"),
    )
    .expect("Failed to parse info_hash");

    let (mut shutdown_tx, shutdown_rx) = shutdown::create_shutdown();
    let ip_source = Box::new(IPV4Consensus::new(2, 10));
    let buckets = |id| -> Box<dyn NodeStorage + Send> { Box::new(NodeBucketStorage::new(id, 8)) };
    let mut settings = dht::DHTSettings::default();
    settings.read_only = true;
    let dht = Arc::new(
        dht::DHT::new(
            shutdown_rx.clone(),
            None,
            port,
            ip_source,
            buckets,
            &ROUTERS,
            settings,
        )
        .await
        .expect("Failed to init DHT"),
    );

    let dht_clone = dht.clone();

    tokio::select! {
        _ = dht.run_event_loop() => {},
        _ = tokio::signal::ctrl_c() => {
            println!("Ctrl+c detected - sending shutdown signal");
            drop(dht);
            drop(shutdown_rx);
            shutdown_tx.shutdown().await;
        },
        _ = async move {
            println!("Starting get_peers");
            let peers = operations::get_peers(&dht_clone, info_hash, 1).await;
            println!("Peers:\n{:?}", peers);
        } => {}
    }
}

use crate::common::ipv4_addr_src::{IPV4AddrSource, IPV4Consensus};
use crate::common::Id;
use crate::dht::{DHTSettings, DHT};
use crate::errors::RustyDHTError;
use crate::shutdown::ShutdownReceiver;
use crate::storage::node_bucket_storage::{NodeBucketStorage, NodeStorage};
use std::net::{Ipv4Addr, SocketAddrV4};

/// Helps to configure and create new [DHT](crate::dht::DHT) instances.
#[derive(Clone)]
pub struct DHTBuilder {
    initial_id: Option<Id>,
    listen_addr: Option<SocketAddrV4>,
    ip_source: Option<Box<dyn IPV4AddrSource + Send>>,
    route_table: Option<Box<dyn NodeStorage + Send>>,
    settings: Option<DHTSettings>,
}

impl DHTBuilder {
    /// Creates a new DHTBuilder
    pub fn new() -> DHTBuilder {
        DHTBuilder {
            initial_id: None,
            listen_addr: None,
            ip_source: None,
            route_table: None,
            settings: None,
        }
    }

    /// Set the initial Id for your DHT node.
    ///
    /// If you don't specify one, DHT will try to generate one based on its
    /// external IPv4 address (if known). If it doesn't know its external
    /// IPv4 address, DHT will generate a random Id and change it later
    /// if it learns that it's not valid for its IPv4 address.
    pub fn initial_id(mut self, id: Id) -> Self {
        self.initial_id = Some(id);
        self
    }

    /// Sets the IPv4 address and port that the DHT should bind its UDP socket to.
    ///
    /// If not specified, it will default to 0.0.0.0:6881
    pub fn listen_addr(mut self, listen_addr: SocketAddrV4) -> Self {
        self.listen_addr = Some(listen_addr);
        self
    }

    /// Provides an IPV4AddrSource implementation to the DHT.
    ///
    /// DHT will use this object to learn its external IPv4 address and keep
    /// up to date when the IP changes. If unspecified, the default implementation
    /// is [IPV4Consensus](crate::common::ipv4_addr_src::IPV4Consensus).
    pub fn ip_source(mut self, ip_source: Box<dyn IPV4AddrSource + Send>) -> Self {
        self.ip_source = Some(ip_source);
        self
    }

    /// Provides a [NodeStorage](crate::storage::node_bucket_storage::NodeStorage)
    /// implementation to the DHT.
    ///
    /// DHT will use this to store its routing tables. If unspecified, the default
    /// implementation is [NodeBucketStorage](crate::storage::node_bucket_storage::NodeBucketStorage)
    /// which works roughly the [BEP0005](http://www.bittorrent.org/beps/bep_0005.html)
    /// describes.
    pub fn route_table(mut self, route_table: Box<dyn NodeStorage + Send>) -> Self {
        self.route_table = Some(route_table);
        self
    }

    /// Provides a DHTSettings object to the DHT.
    ///
    /// DHTSettings can be built with [DHTSettingsBuilder](crate::dht::DHTSettingsBuilder)
    /// but the defaults should work fine in most cases.
    pub fn settings(mut self, settings: DHTSettings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Build a DHT
    ///
    /// This must be called from within a tokio Runtime context because it constructs
    /// a tokio UdpSocket. See [tokio::net::UdpSocket].
    pub fn build(self, shutdown_rx: ShutdownReceiver) -> Result<DHT, RustyDHTError> {
        DHT::new(
            shutdown_rx,
            self.initial_id,
            std::net::SocketAddr::V4(
                self.listen_addr
                    .unwrap_or_else(|| SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 6881)),
            ),
            self.ip_source
                .unwrap_or_else(|| Box::new(IPV4Consensus::new(2, 10))),
            self.route_table
                .unwrap_or_else(|| Box::new(NodeBucketStorage::new(Id::ZERO, 8))),
            self.settings.unwrap_or_else(|| DHTSettings::default()),
        )
    }
}

/// Struct that represents configuration for DHT that, in general, does
/// not change after the DHT is started.
///
/// You'll probably want one of these to pass into [DHT::new()](crate::dht::DHT::new).
///
/// Use [DHTSettings::default()](crate::dht::DHTSettings::default) to create an instance with the
/// 'recommended' defaults (which can be customized). Or instantiate your own
/// with `let settings = DHTSettings {/* your settings here */};`
#[derive(Clone)]
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

    /// If true, we will set the read only flag in outgoing requests to prevent
    /// other nodes from adding us to their routing tables. This is useful if
    /// we're behind a restrictive NAT/firewall and can't accept incoming
    /// packets from IPs that we haven't sent anything to.
    pub read_only: bool,

    /// Vector of hostnames/ports that the DHT will use as DHT routers for
    /// bootstrapping purposes.
    ///
    /// E.g., "router.example.org:6881"
    pub routers: Vec<String>,
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
            read_only: false,
            routers: vec![
                "router.bittorrent.com:6881".to_string(),
                "router.utorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
            ],
        }
    }
}

impl Default for DHTSettings {
    fn default() -> Self {
        Self::default()
    }
}

#[derive(Clone, Default)]
/// Builder for DHTSettings
pub struct DHTSettingsBuilder {
    settings: DHTSettings,
}

macro_rules! make_builder_method {
    ($prop:ident, $prop_type:ty) => {
        pub fn $prop(mut self, $prop: $prop_type) -> Self {
            self.settings.$prop = $prop;
            self
        }
    };
}

impl DHTSettingsBuilder {
    pub fn new() -> DHTSettingsBuilder {
        Self::default()
    }

    make_builder_method!(token_secret_size, usize);
    make_builder_method!(max_peers_response, usize);
    make_builder_method!(max_sample_response, usize);
    make_builder_method!(min_sample_interval_secs, i32);
    make_builder_method!(router_ping_interval_secs, u64);
    make_builder_method!(reverify_interval_secs, u64);
    make_builder_method!(reverify_grace_period_secs, u64);
    make_builder_method!(verify_grace_period_secs, u64);
    make_builder_method!(get_peers_freshness_secs, u64);
    make_builder_method!(find_nodes_interval_secs, u64);
    make_builder_method!(find_nodes_skip_count, usize);
    make_builder_method!(max_torrents, usize);
    make_builder_method!(max_peers_per_torrent, usize);
    make_builder_method!(ping_check_interval_secs, u64);
    make_builder_method!(outgoing_request_prune_secs, u64);
    make_builder_method!(outgoing_reqiest_check_interval_secs, u64);
    make_builder_method!(read_only, bool);
    make_builder_method!(routers, Vec<String>);

    pub fn build(self) -> DHTSettings {
        self.settings
    }
}

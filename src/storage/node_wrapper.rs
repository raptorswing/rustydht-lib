use super::buckets::Bucketable;
use crate::common::{Id, Node};
use std::time::Instant;

/// Wraps a Node with information about when the DHT first saw, last saw it, and last verified it.
/// This is used by [NodeStorage](crate::storage::node_bucket_storage::NodeStorage) implementations
#[derive(Debug, Clone)]
pub struct NodeWrapper {
    pub node: Node,

    /// The Instant when this Node was first seen by us on the network
    pub first_seen: std::time::Instant,

    /// The Instant when this Node was last marked seen by us on the network.
    pub last_seen: std::time::Instant,

    /// The Instant when this Node was most recently marked as being alive on the network.
    /// In general, being "verified" requires that the Node has responded to a request
    /// from us recently.
    pub last_verified: Option<std::time::Instant>,
}

impl NodeWrapper {
    /// Creates a new NodeWrapper.
    ///
    /// Initializes `first_seen` and `last_seen` to the current time. Initializes `last_verified` to `None`.
    pub fn new(node: Node) -> NodeWrapper {
        let now = std::time::Instant::now();
        NodeWrapper {
            node: node,
            first_seen: now,
            last_seen: now,
            last_verified: None,
        }
    }
}

impl Bucketable for NodeWrapper {
    fn get_id(&self) -> Id {
        self.node.id
    }

    fn get_first_seen(&self) -> Instant {
        self.first_seen
    }
}

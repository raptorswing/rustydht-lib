use super::buckets::Bucketable;
use crate::common::{Id, Node};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct NodeWrapper {
    pub node: Node,
    pub first_seen: std::time::Instant,
    pub last_seen: std::time::Instant,
    pub last_verified: Option<std::time::Instant>,
}

impl NodeWrapper {
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

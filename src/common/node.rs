use super::Id;
use std::net::SocketAddr;

#[derive(Debug, PartialEq, Clone)]

/// Represents a Node on the DHT network. A node has an [Id](crate::common::Id) and a [SocketAddr](std::net::SocketAddr).
pub struct Node {
    pub id: Id,
    pub address: SocketAddr,
}

impl Node {
    /// Creates a new Node from an id and socket address.
    pub fn new(id: Id, address: SocketAddr) -> Node {
        Node {
            id,
            address,
        }
    }
}

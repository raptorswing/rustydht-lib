use super::Id;
use std::net::SocketAddr;

#[derive(Debug, PartialEq, Clone)]
pub struct Node {
    pub id: Id,
    pub address: SocketAddr,
}

impl Node {
    pub fn new(id: Id, address: SocketAddr) -> Node {
        Node {
            id: id,
            address: address,
        }
    }
}

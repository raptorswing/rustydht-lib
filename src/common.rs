use anyhow::anyhow;
use std::net::SocketAddr;

use crate::errors::RustyDHTError;

#[derive(Debug, PartialEq)]
pub struct Id<const LENGTH: usize> {
    bytes: [u8; LENGTH],
}

impl<const LENGTH: usize> Id<LENGTH> {
    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Id<LENGTH>, RustyDHTError> {
        let bytes = bytes.as_ref();
        if bytes.len() != LENGTH {
            return Err(RustyDHTError::PacketParseError(anyhow!(
                "Wrong number of bytes"
            )));
        }

        let mut tmp: [u8; LENGTH] = [0; LENGTH];
        for i in 0..LENGTH {
            tmp[i] = bytes[i];
        }

        Ok(Id { bytes: tmp })
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    #[cfg(test)]
    pub fn from_hex(h: &str) -> Result<Id<LENGTH>, RustyDHTError> {
        let bytes =
            hex::decode(h).map_err(|hex_err| RustyDHTError::PacketParseError(hex_err.into()))?;

        Id::from_bytes(&bytes)
    }
}

#[derive(Debug, PartialEq)]
pub struct Node<const LENGTH: usize> {
    pub id: Id<LENGTH>,
    pub address: SocketAddr,
}

impl<const LENGTH: usize> Node<LENGTH> {
    pub fn new(id: Id<LENGTH>, address: SocketAddr) -> Node<LENGTH> {
        Node {
            id: id,
            address: address,
        }
    }
}

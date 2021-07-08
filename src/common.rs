use anyhow::anyhow;

extern crate crc;
use crc::crc32;

use rand::prelude::*;

use std::net::{SocketAddr, IpAddr};
use std::convert::TryInto;

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

    pub fn from_ip(ip: &IpAddr) -> Id<LENGTH> {
        if LENGTH < 3 {
            // TODO cleanup when const generics support specialization, I guess...
            panic!("This function requires id of at least 3 bytes");
        }
        let mut rng = thread_rng();
        let r: u8 = rng.gen();

        let magic_prefix = IdPrefixMagic::from_ip(&ip, r);

        let mut bytes = [0; LENGTH];

        bytes[0] = magic_prefix.prefix[0];
        bytes[1] = magic_prefix.prefix[1];
        bytes[2] = (magic_prefix.prefix[2] & 0xf8) | (rng.gen::<u8>() & 0x7);
        for i in 3..LENGTH-1 {
            bytes[i] = rng.gen();
        }
        bytes[LENGTH-1] = r;

        return Id { bytes: bytes };
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    pub fn is_valid_for_ip(&self, ip: &IpAddr) -> bool {
        // TODO return true if ip is not globally routable
        let expected = IdPrefixMagic::from_ip(ip, *self.bytes.last().expect("Zero length id not supported. Shame on you"));
        let actual = IdPrefixMagic::from_id(&self);

        return expected == actual;
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

#[derive(Debug)]
struct IdPrefixMagic {
    prefix: [u8; 3],
    suffix: u8,
}

impl IdPrefixMagic {
    // Populates an IdPrefixMagic from the bytes of an id.
    // This isn't a way to generate a valid IdPrefixMagic from a random id.
    // For that, use MainlineId::from_ip
    fn from_id<const LENGTH: usize>(id: &Id<LENGTH>) -> IdPrefixMagic {
        if LENGTH < 3 {
            // TODO cleanup when const generics support specialization, I guess...
            panic!("This function requires id of at least 3 bytes");
        }

        return IdPrefixMagic {
            prefix: id.bytes[..3]
                .try_into()
                .expect("Failed to grab first three bytes of id"),
            suffix: *id.bytes.last().expect("Zero length id not supported. Shame on you"),
        };
    }

    fn from_ip(ip: &IpAddr, seed_r: u8) -> IdPrefixMagic {
        match ip {
            IpAddr::V4(ipv4) => {
                let r32: u32 = seed_r.into();
                let magic: u32 = 0x030f3fff;
                let ip_int: u32 = u32::from_be_bytes(ipv4.octets());
                let nonsense: u32 = (ip_int & magic) | (r32 << 29);
                let crc: u32 = crc32::checksum_castagnoli(&nonsense.to_be_bytes());
                return IdPrefixMagic {
                    prefix: crc.to_be_bytes()[..3]
                        .try_into()
                        .expect("Failed to convert bytes 0-2 of the crc into a 3-byte array"),
                    suffix: seed_r,
                };
            }
            IpAddr::V6(ipv6) => {
                let r64: u64 = seed_r.into();
                let magic: u64 = 0x0103070f1f3f7fff;
                let ip_int: u64 = u64::from_be_bytes(
                    ipv6.octets()[8..]
                        .try_into()
                        .expect("Failed to get IPv6 bytes"),
                );
                let nonsense: u64 = ip_int & magic | (r64 << 61);
                let crc: u32 = crc32::checksum_castagnoli(&nonsense.to_be_bytes());
                return IdPrefixMagic {
                    prefix: crc.to_be_bytes()[..2].try_into().expect("Failed to poop"),
                    suffix: seed_r,
                };
            }
        };
    }
}

impl PartialEq for IdPrefixMagic {
    fn eq(&self, other: &Self) -> bool {
        return self.prefix[0] == other.prefix[0]
            && self.prefix[1] == other.prefix[1]
            && self.prefix[2] & 0xf8 == other.prefix[2] & 0xf8
            && self.suffix == other.suffix;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_from_ip_v4() {
        assert_eq!(
            IdPrefixMagic::from_ip(&IpAddr::V4(Ipv4Addr::new(124, 31, 75, 21)), 1),
            IdPrefixMagic {
                prefix: [0x5f, 0xbf, 0xbf],
                suffix: 1
            }
        );
        assert_eq!(
            IdPrefixMagic::from_ip(&IpAddr::V4(Ipv4Addr::new(21, 75, 31, 124)), 86),
            IdPrefixMagic {
                prefix: [0x5a, 0x3c, 0xe9],
                suffix: 0x56
            }
        );

        assert_eq!(
            IdPrefixMagic::from_ip(&IpAddr::V4(Ipv4Addr::new(65, 23, 51, 170)), 22),
            IdPrefixMagic {
                prefix: [0xa5, 0xd4, 0x32],
                suffix: 0x16
            }
        );

        assert_eq!(
            IdPrefixMagic::from_ip(&IpAddr::V4(Ipv4Addr::new(84, 124, 73, 14)), 65),
            IdPrefixMagic {
                prefix: [0x1b, 0x03, 0x21],
                suffix: 0x41
            }
        );

        assert_eq!(
            IdPrefixMagic::from_ip(&IpAddr::V4(Ipv4Addr::new(43, 213, 53, 83)), 90),
            IdPrefixMagic {
                prefix: [0xe5, 0x6f, 0x6c],
                suffix: 0x5a
            }
        );
    }

    #[test]
    fn test_generate_valid_id() {
        let ip = IpAddr::V4(Ipv4Addr::new(124, 31, 75, 21));
        let id = Id::<20>::from_ip(&ip);
        assert!(id.is_valid_for_ip(&ip));
    }
}
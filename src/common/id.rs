use anyhow::anyhow;

extern crate crc;
use crc::crc32;

use rand::prelude::*;

use std::convert::TryInto;
use std::net::IpAddr;

use crate::errors::RustyDHTError;

/// The length (in bytes) of BitTorrent info hashes and DHT node ids.
pub const ID_SIZE: usize = 20;

/// Represents the id of a [Node](crate::common::Node) or a BitTorrent info-hash. Basically, it's a
/// 20-byte identifier.
#[derive(Eq, PartialEq, Copy, Clone)]
pub struct Id {
    bytes: [u8; ID_SIZE],
}

impl Id {
    /// Create a new Id from some bytes. Returns Err if `bytes` is not of length
    /// [ID_SIZE](crate::common::ID_SIZE).
    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Id, RustyDHTError> {
        let bytes = bytes.as_ref();
        if bytes.len() != ID_SIZE {
            return Err(RustyDHTError::PacketParseError(anyhow!(
                "Wrong number of bytes"
            )));
        }

        let mut tmp: [u8; ID_SIZE] = [0; ID_SIZE];
        for i in 0..ID_SIZE {
            tmp[i] = bytes[i];
        }

        Ok(Id { bytes: tmp })
    }

    /// Generates a random Id for a mainline DHT node with the provided IP address.
    /// The generated Id will be valid with respect to [BEP0042](http://bittorrent.org/beps/bep_0042.html).
    pub fn from_ip(ip: &IpAddr) -> Id {
        let mut rng = thread_rng();
        let r: u8 = rng.gen();

        let magic_prefix = IdPrefixMagic::from_ip(&ip, r);

        let mut bytes = [0; ID_SIZE];

        bytes[0] = magic_prefix.prefix[0];
        bytes[1] = magic_prefix.prefix[1];
        bytes[2] = (magic_prefix.prefix[2] & 0xf8) | (rng.gen::<u8>() & 0x7);
        for i in 3..ID_SIZE - 1 {
            bytes[i] = rng.gen();
        }
        bytes[ID_SIZE - 1] = r;

        return Id { bytes: bytes };
    }

    /// Generates a completely random Id. The returned Id is *not* guaranteed to be
    /// valid with respect to [BEP0042](http://bittorrent.org/beps/bep_0042.html).
    pub fn from_random<T: rand::RngCore>(rng: &mut T) -> Id {
        let mut bytes: [u8; ID_SIZE] = [0; ID_SIZE];
        rng.fill_bytes(&mut bytes);

        return Id { bytes: bytes };
    }

    /// Copies the byes that make up the Id and returns them in a Vec
    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    /// Evaluates the Id and decides if it's a valid Id for a DHT node with the
    /// provided IP address (based on [BEP0042](http://bittorrent.org/beps/bep_0042.html)).
    /// Note: the current implementation does not handle non-globally-routable address space
    /// properly. It will likely return false for non-routable IPv4 address space (against the spec).
    pub fn is_valid_for_ip(&self, ip: &IpAddr) -> bool {
        // TODO return true if ip is not globally routable
        if ip.is_loopback() {
            return true;
        }
        let expected = IdPrefixMagic::from_ip(ip, self.bytes[ID_SIZE - 1]);
        let actual = IdPrefixMagic::from_id(&self);

        return expected == actual;
    }

    /// Returns the number of bits of prefix that the two ids have in common.
    ///
    /// Consider two Ids with binary values `10100000` and `10100100`. This function
    /// would return `5` because the Ids share the common 5-bit prefix `10100`.
    pub fn matching_prefix_bits(&self, other: &Self) -> usize {
        let xored = self.xor(&other);
        let mut to_ret: usize = 0;

        for i in 0..ID_SIZE {
            let leading_zeros: usize = xored.bytes[i]
                .leading_zeros()
                .try_into()
                .expect("this should never fail");
            to_ret = to_ret + leading_zeros;

            if leading_zeros < 8 {
                break;
            }
        }

        return to_ret;
    }

    /// Creates an Id from a hex string.
    ///
    /// For example: `let id = Id::from_hex("88ffb73943354a00dc2dadd14c54d28020a513c8").unwrap();`
    pub fn from_hex(h: &str) -> Result<Id, RustyDHTError> {
        let bytes =
            hex::decode(h).map_err(|hex_err| RustyDHTError::PacketParseError(hex_err.into()))?;

        Id::from_bytes(&bytes)
    }

    /// Computes the exclusive or (XOR) of this Id with another. The BitTorrent DHT
    /// uses XOR as its distance metric.
    ///
    /// Example: `let distance_between_nodes = id.xor(other_id);`
    pub fn xor(&self, other: &Id) -> Id {
        let mut bytes: [u8; ID_SIZE] = [0; ID_SIZE];
        for i in 0..ID_SIZE {
            bytes[i] = self.bytes[i] ^ other.bytes[i];
        }

        Id::from_bytes(&bytes).expect("Wrong number of bytes for id")
    }

    /// Makes a new id that's similar to this one.
    /// `identical_bytes` specifies how many bytes of the resulting Id should be the same as `this`.
    /// `identical_bytes` must be in the range (0, [ID_SIZE](crate::common::ID_SIZE)) otherwise Err
    /// is returned.
    pub fn make_mutant(&self, identical_bytes: usize) -> Result<Id, RustyDHTError> {
        if identical_bytes <= 0 || identical_bytes >= ID_SIZE {
            return Err(RustyDHTError::GeneralError(anyhow!(
                "identical_bytes must be in range (0, ID_SIZE)"
            )));
        }
        let mut mutant = Id::from_random(&mut thread_rng());
        for i in 0..identical_bytes {
            mutant.bytes[i] = self.bytes[i];
        }
        Ok(mutant)
    }

    /// An Id with all of its bits set to 0
    pub const ZERO: Self = Id {
        bytes: [0; ID_SIZE],
    };
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.bytes))
    }
}

impl std::fmt::Debug for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.bytes))
    }
}

impl std::hash::Hash for Id {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl PartialOrd for Id {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        for i in 0..self.bytes.len() {
            if self.bytes[i] < other.bytes[i] {
                return Some(std::cmp::Ordering::Less);
            } else if self.bytes[i] > other.bytes[i] {
                return Some(std::cmp::Ordering::Greater);
            }
        }

        Some(std::cmp::Ordering::Equal)
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
    // For that, use Id::from_ip
    fn from_id(id: &Id) -> IdPrefixMagic {
        return IdPrefixMagic {
            prefix: id.bytes[..3]
                .try_into()
                .expect("Failed to grab first three bytes of id"),
            suffix: id.bytes[ID_SIZE - 1],
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
        let id = Id::from_ip(&ip);
        assert!(id.is_valid_for_ip(&ip));
    }

    #[test]
    fn test_id_xor() {
        let h1 = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        let h2 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let h3 = h1.xor(&h2);
        assert!(h3 == h1);

        let h1 = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        let h2 = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        let h3 = h1.xor(&h2);
        assert!(h3 == Id::from_hex("0000000000000000000000000000000000000000").unwrap());

        let h1 = Id::from_hex("1010101010101010101010101010101010101010").unwrap();
        let h2 = Id::from_hex("0101010101010101010101010101010101010101").unwrap();
        let h3 = h1.xor(&h2);
        assert!(h3 == Id::from_hex("1111111111111111111111111111111111111111").unwrap());

        let h1 = Id::from_hex("fefefefefefefefefefefefefefefefefefefefe").unwrap();
        let h2 = Id::from_hex("0505050505050505050505050505050505050505").unwrap();
        let h3 = h1.xor(&h2);
        assert!(h3 == Id::from_hex("fbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfb").unwrap());
    }

    #[test]
    fn test_id_ordering() {
        let h1 = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        let h2 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        assert!(h1 > h2);
        assert!(h2 < h1);
        assert!(h1 != h2);

        let h1 = Id::from_hex("00000000000000000000f0000000000000000000").unwrap();
        let h2 = Id::from_hex("000000000000000000000fffffffffffffffffff").unwrap();
        assert!(h1 > h2);
        assert!(h2 < h1);
        assert!(h1 != h2);

        let h1 = Id::from_hex("1000000000000000000000000000000000000000").unwrap();
        let h2 = Id::from_hex("0fffffffffffffffffffffffffffffffffffffff").unwrap();
        assert!(h1 > h2);
        assert!(h2 < h1);
        assert!(h1 != h2);

        let h1 = Id::from_hex("1010101010101010101010101010101010101010").unwrap();
        let h2 = Id::from_hex("1010101010101010101010101010101010101010").unwrap();
        assert!(!(h1 > h2));
        assert!(!(h2 > h1));
        assert!(h1 == h2);

        let h1 = Id::from_hex("0000000000000000000000000000000000000010").unwrap();
        let h2 = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        assert!(h1 > h2);
        assert!(h2 < h1);
        assert!(h1 != h2);
    }

    #[test]
    fn test_matching_prefix_bits() {
        let h1 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let h2 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        assert_eq!(h1.matching_prefix_bits(&h2), 160);

        let h1 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let h2 = Id::from_hex("f000000000000000000000000000000000000000").unwrap();
        assert_eq!(h1.matching_prefix_bits(&h2), 0);

        let h1 = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let h2 = Id::from_hex("1000000000000000000000000000000000000000").unwrap();
        assert_eq!(h1.matching_prefix_bits(&h2), 3);
    }
}

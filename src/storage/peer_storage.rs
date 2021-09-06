use std::cell::RefCell;
use std::net::SocketAddr;

use crate::common::Id;

use lru::LruCache;

use log::debug;

pub struct PeerInfo {
    addr: SocketAddr,
    last_updated: std::time::Instant,
}

impl PeerInfo {
    fn new(addr: SocketAddr) -> PeerInfo {
        PeerInfo {
            addr: addr,
            last_updated: std::time::Instant::now(),
        }
    }
}

pub struct PeerStorage {
    /// Stored in RefCell to allow interior mutability... LruCache mutates on .get()
    peers: RefCell<LruCache<Id, LruCache<SocketAddr, PeerInfo>>>,
    max_peers_per_torrent: usize,
}

impl PeerStorage {
    pub fn new(max_torrents: usize, max_peers_per_torrent: usize) -> PeerStorage {
        PeerStorage {
            peers: RefCell::new(LruCache::new(max_torrents)),
            max_peers_per_torrent: max_peers_per_torrent,
        }
    }

    pub fn announce_peer(&mut self, info_hash: Id, peer_addr: SocketAddr) {
        let mut peers = self.peers.borrow_mut();
        match peers.get_mut(&info_hash) {
            Some(swarm_lru) => {
                swarm_lru.put(peer_addr, PeerInfo::new(peer_addr));
            }

            None => {
                let mut swarm_lru = LruCache::new(self.max_peers_per_torrent);
                swarm_lru.put(peer_addr, PeerInfo::new(peer_addr));
                peers.put(info_hash, swarm_lru);
            }
        }
        debug!(target: "PeerStorage", "{} is in swarm with info_hash {}", peer_addr, info_hash);
    }

    pub fn get_peers(
        &self,
        info_hash: &Id,
        newer_than: Option<std::time::Instant>,
    ) -> Vec<SocketAddr> {
        let mut peers = self.peers.borrow_mut();
        let mut to_ret = Vec::new();
        if let Some(swarm_lru) = peers.get(info_hash) {
            let mut tmp = swarm_lru
                .iter()
                .filter(|pi| pi.0.ip().is_ipv4()) // Only return IPv4 for now, you dog!
                .filter(|pi| newer_than.is_none() || pi.1.last_updated > newer_than.unwrap())
                .map(|pi| pi.0.clone())
                .collect();
            to_ret.append(&mut tmp);
        }

        to_ret
    }

    pub fn get_info_hashes(&self) -> Vec<Id> {
        let peers = self.peers.borrow();
        peers.iter().map(|kv| kv.0.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let mut storage = PeerStorage::new(1, 2);

        // It's empty to start
        assert_eq!(0, storage.get_info_hashes().len());

        // Add an info_hash with peer
        let info_hash =
            Id::from_hex("1988091919880919198809191988091919880919").expect("Couldn't make Id");
        let peer1 = "10.0.0.6:1234".parse().expect("Couldn't make SocketAddr");

        storage.announce_peer(info_hash, peer1);

        // Now there's one info_hash and we can read back the peer
        let hashes = storage.get_info_hashes();
        assert_eq!(1, hashes.len());
        assert_eq!(info_hash, hashes[0]);
        let peers = storage.get_peers(&info_hash, None);
        assert_eq!(1, peers.len());
        assert_eq!(peer1, peers[0]);
    }

    // Test that the max_torrents limit is honored
    #[test]
    fn test_max_torrents_limited() {
        let mut storage = PeerStorage::new(1, 2);

        // Announce for first info_hash
        let info_hash =
            Id::from_hex("1988091919880919198809191988091919880919").expect("Couldn't make Id");
        let peer1 = "10.0.0.6:1234".parse().expect("Couldn't make SocketAddr");
        storage.announce_peer(info_hash, peer1);

        // Announce for second info_hash
        let info_hash2 =
            Id::from_hex("2088091919880919198809191988091919880920").expect("Couldn't make Id");
        let peer2 = "10.0.0.7:1234".parse().expect("Couldn't make SocketAddr");
        storage.announce_peer(info_hash2, peer2);

        // There can only be one - the second one
        let hashes = storage.get_info_hashes();
        assert_eq!(1, hashes.len());
        assert_eq!(info_hash2, hashes[0]);
        let peers = storage.get_peers(&info_hash2, None);
        assert_eq!(1, peers.len());
        assert_eq!(peer2, peers[0]);
    }

    #[test]
    fn test_max_peers_limited() {
        let mut storage = PeerStorage::new(10, 1);

        let info_hash =
            Id::from_hex("1988091919880919198809191988091919880919").expect("Couldn't make Id");
        let peer1 = "10.0.0.6:1234".parse().expect("Couldn't make SocketAddr");
        let peer2 = "10.0.0.7:1234".parse().expect("Couldn't make SocketAddr");
        storage.announce_peer(info_hash, peer1);
        storage.announce_peer(info_hash, peer2);

        // There can only be one - the second one
        let peers = storage.get_peers(&info_hash, None);
        assert_eq!(1, peers.len());
        assert_eq!(peer2, peers[0]);
    }

    #[test]
    fn test_get_peers_newer_than() {
        let mut storage = PeerStorage::new(1, 3);
        let info_hash =
            Id::from_hex("1988091919880919198809191988091919880919").expect("Couldn't make Id");
        let peer1 = "10.0.0.6:1234".parse().expect("Couldn't make SocketAddr");
        let peer2 = "10.0.0.7:1234".parse().expect("Couldn't make SocketAddr");
        let peer3 = "10.0.0.8:1234".parse().expect("Couldn't make SocketAddr");
        storage.announce_peer(info_hash, peer1);
        let newer_than = std::time::Instant::now();
        storage.announce_peer(info_hash, peer2);
        storage.announce_peer(info_hash, peer3);

        let peers = storage.get_peers(&info_hash, Some(newer_than));
        assert_eq!(2, peers.len());
        assert!(peers.contains(&peer2));
        assert!(peers.contains(&peer3));
    }
}

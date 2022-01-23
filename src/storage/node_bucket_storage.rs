use super::buckets::Buckets;
use super::node_wrapper::NodeWrapper;
use crate::common::{Id, Node};
use dyn_clone::DynClone;
use std::time::{Duration, Instant};

use log::trace;

/// Trait for things that can store DHT nodes
///
/// [DHT](crate::dht::DHT) uses an object implementing this trait to keep
/// track of other nodes on the network. The default implementation is
/// [NodeBucketStorage](crate::storage::node_bucket_storage::NodeBucketStorage)
/// but DHT can accept any object that implements this trait.
pub trait NodeStorage: DynClone + Send {
    /// Add a Node to storage, or update the record of a Node already in storage.
    ///
    /// # Parameters
    /// * `node` - The Node to add or update
    /// * `verified` - Set true if the caller knows the Node is live and can communicate
    /// with it (i.e. we've sent a request and received a response). If true, the
    /// `last_verified` and `last_seen` properties of
    /// [NodeWrapper](crate::storage::node_wrapper::NodeWrapper) will be updated.
    /// If false, only `last_seen` will be updated.
    fn add_or_update(&mut self, node: Node, verified: bool);

    /// Erase all Nodes from storage, resetting this object to its empty state
    fn clear(&mut self);

    /// Return the number of Nodes that have been verified (`last_verified`
    /// property is not None) and the number that haven't.
    ///
    /// Returned as a tuple of `(unverified count, verified count)`
    fn count(&self) -> (usize, usize);

    /// Return a copy of the records for all the unverified Nodes
    fn get_all_unverified(&self) -> Vec<NodeWrapper>;

    /// Return a copy of the records for all verified Nodes
    fn get_all_verified(&self) -> Vec<NodeWrapper>;

    /// Return a copy of the nearest nodes to the provided Id.
    ///
    /// # Parameters
    /// * `id` - the target id. Nodes that are near this Id (based on
    /// XOR distance metric) should be returned.
    /// * `exclude` - If there's a Node that you know will be near
    /// `id` and you don't want to see it, you can exclude it by
    /// providing its Id here.
    fn get_nearest_nodes(&self, id: &Id, exclude: Option<&Id>) -> Vec<Node>;

    /// Prune (remove) records of Nodes tht haven't been seen/verified recently.node_wrapper
    ///
    /// # Parameters
    /// * `grace_period` - Previously verified Nodes are dropped if their `last_verified`
    /// property is None or less than `(now - grace_period)`.
    /// * `unverified_grace_period` -  Previously seen (but not verified) Nodes are dropped
    /// if their `last_seen` property is less than `(now - unverified_grace_period)`
    fn prune(&mut self, grace_period: Duration, unverified_grace_period: Duration);

    /// Set our own DHT node's Id.
    ///
    /// This is used by implementations that use XOR distance from our own Id as part of
    /// their internal storage algorithm (the normal DHT bucket storage does this). But
    /// implementations are free to **ignore** this if they want.
    ///
    /// Some implementations may reset
    /// (similar to invoking [clear()](crate::storage::node_bucket_storage::NodeStorage::clear))
    /// when this method is called.
    fn set_id(&mut self, our_id: Id);
}

dyn_clone::clone_trait_object!(NodeStorage);

/// Implements the XOR distance-based bucketing system described in
/// [BEP0005](http://www.bittorrent.org/beps/bep_0005.html) (more or less).
///
/// NodeBucketStorage keeps one bucket with capacity `k` for each bit of the DHT's
/// info hashes. Since the mainline DHT uses 20 byte (160 bit) info hashes, this
/// object will by default keep a maximum of 160 buckets of size `k`.
///
/// As described in BEP0005, the buckets are organized according to an XOR distance metric
/// so that Nodes with ids closer to our DHT's Id are given the most storage space.
/// The first bucket stores Nodes that have a zero (or more) bit prefix in common with the DHT Id.
/// The second bucket stores Nodes that have a one (or more) bit prefix in common with the DHT Id.
/// Two or more bits in the third. Three or more bits in the 4th. And so on.
#[derive(Clone)]
pub struct NodeBucketStorage {
    verified: Buckets<NodeWrapper>,
    unverified: Buckets<NodeWrapper>,
}

impl NodeBucketStorage {
    /// Create a new NodeBucketStorage.
    ///
    /// # Parameters
    /// * `our_id` - the current Id of the DHT node that will use this object for storage. Nodes will
    /// be assigned to buckets based on the XOR distance between their Id and this one.
    /// * `k` - the number of nodes that can be stored in a bucket.
    pub fn new(our_id: Id, k: usize) -> NodeBucketStorage {
        NodeBucketStorage {
            verified: Buckets::new(our_id, k),
            unverified: Buckets::new(our_id, k),
        }
    }

    fn add_or_update_last_seen(&mut self, node: Node) {
        if let Some(existing) = self.verified.get_mut(&node.id) {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Updating existing verified {:?} last seen", node);
            existing.last_seen = std::time::Instant::now();
        } else if let Some(existing) = self.unverified.get_mut(&node.id) {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Updating existing unverified {:?} last seen", node);
            existing.last_seen = std::time::Instant::now();
        } else {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Attempting to add unverified {:?}", node);
            self.unverified.add(NodeWrapper::new(node), None);
        }
    }

    fn add_or_update_verified(&mut self, node: Node) {
        let now = std::time::Instant::now();

        // Already exists in unverified.
        // Remove it and try to add it to Verified.
        // If verified is full, add whatever overflows back to unverified (if it fits)
        if let Some(mut item) = self.unverified.remove(&node.id) {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Attempting to move {:?} from unverified to verified", node);
            item.last_seen = now;
            item.last_verified = Some(now);
            let mut chump_list = Vec::with_capacity(1);
            self.verified.add(item, Some(&mut chump_list));

            for item in chump_list {
                self.unverified.add(item, None);
            }
        }
        // Already exists in verified.
        // Update it
        else if let Some(wrapper) = self.verified.get_mut(&node.id) {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Marking verified {:?} as verified again", node);
            wrapper.last_verified = Some(wrapper.last_seen);
            wrapper.last_seen = now;
            wrapper.last_verified = Some(now);
        }
        // Doesn't exist yet
        else {
            trace!(target: "rustydht_lib::NodeBucketStorage", "Marking new {:?} as verified", node);
            let mut wrapper = NodeWrapper::new(node);
            wrapper.last_seen = now;
            wrapper.last_verified = Some(now);

            let mut chump_list = Vec::with_capacity(1);
            self.verified.add(wrapper, Some(&mut chump_list));

            for item in chump_list {
                self.unverified.add(item, None);
            }
        }
    }
}

impl NodeStorage for NodeBucketStorage {
    fn add_or_update(&mut self, node: Node, verified: bool) {
        if verified {
            self.add_or_update_verified(node);
        } else {
            self.add_or_update_last_seen(node);
        }
    }

    fn clear(&mut self) {
        self.unverified.clear();
        self.verified.clear();
    }

    fn count(&self) -> (usize, usize) {
        (self.unverified.count(), self.verified.count())
    }

    fn get_all_unverified(&self) -> Vec<NodeWrapper> {
        self.unverified
            .values()
            .iter()
            .copied().cloned()
            .collect()
    }

    fn get_all_verified(&self) -> Vec<NodeWrapper> {
        self.verified
            .values()
            .iter()
            .copied().cloned()
            .collect()
    }

    fn get_nearest_nodes(&self, id: &Id, exclude: Option<&Id>) -> Vec<Node> {
        self.verified
            .get_nearest_nodes(id, exclude)
            .iter()
            .map(|nw| nw.node.clone())
            .collect()
    }

    fn prune(&mut self, grace_period: Duration, unverified_grace_period: Duration) {
        if let Some(time) = Instant::now().checked_sub(grace_period) {
            if let Some(unverified_time) = Instant::now().checked_sub(unverified_grace_period) {
                self.verified.retain(|nw| {
                    if let Some(last_verified) = nw.last_verified {
                        return last_verified >= time;
                    }
                    trace!(target: "rustydht_lib::NodeBucketStorage", "Verified {:?} hasn't verified recently. Removing.", nw.node);
                    false
                });
                self.unverified.retain(|nw| {
                    if let Some(last_verified) = nw.last_verified {
                        if last_verified >= time {
                            return true;
                        }
                    }
                    if nw.last_seen >= time && nw.last_seen >= unverified_time {
                        return true;
                    }
                    trace!(target: "rustydht_lib::NodeBucketStorage", "Unverified {:?} is dead. Removing", nw.node);
                    false
                });
            }
        }
    }

    fn set_id(&mut self, new_id: Id) {
        self.verified.set_id(new_id);
        self.unverified.set_id(new_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    #[test]
    fn test_add_unverified_and_verified() {
        for test_verified in [false, true] {
            let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
            let mut storage = NodeBucketStorage::new(our_id, 8);

            let node1 = Node::new(
                Id::from_hex("1234567812345678123456781234567812345678").unwrap(),
                "1.2.3.4:1234".parse().unwrap(),
            );
            let node2 = Node::new(
                Id::from_hex("5234567812345678123456781234567812345678").unwrap(),
                "5.2.3.4:1234".parse().unwrap(),
            );

            storage.add_or_update(node1.clone(), test_verified);
            storage.add_or_update(node2.clone(), test_verified);

            let (expected_unverified, expected_verified) = match test_verified {
                true => (0, 2),
                false => (2, 0),
            };
            let unverified = storage.get_all_unverified();
            let verified = storage.get_all_verified();
            assert_eq!(expected_unverified, unverified.len());
            assert_eq!(expected_verified, verified.len());
            let counts = storage.count();
            assert_eq!((expected_unverified, expected_verified), counts);

            let populated_one = match test_verified {
                true => &verified,
                false => &unverified,
            };

            let node1_wrapper = populated_one.iter().find(|nw| nw.node == node1).unwrap();
            let node2_wrapper = populated_one.iter().find(|nw| nw.node == node1).unwrap();

            assert_eq!(test_verified, node1_wrapper.last_verified.is_some());
            assert_eq!(test_verified, node2_wrapper.last_verified.is_some());
        }
    }

    #[test]
    fn test_node_lifecycle() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = NodeBucketStorage::new(our_id, 8);

        let node1 = Node::new(
            Id::from_hex("1234567812345678123456781234567812345678").unwrap(),
            "1.2.3.4:1234".parse().unwrap(),
        );
        // Add an unverified node
        storage.add_or_update(node1.clone(), false);
        let before_update = std::time::Instant::now();

        // Mark the node as seen again (but not verified)
        storage.add_or_update(node1.clone(), false);
        let wrapper = &storage.get_all_unverified()[0];

        // verify last_seen was updated, but still not verified
        assert!(wrapper.last_seen >= before_update);
        assert!(wrapper.last_verified.is_none());

        // Mark the node verified
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1.clone(), true);

        let wrapper = &storage.get_all_verified()[0];

        // verify it's verified and last_seen updated again
        assert!(wrapper.last_verified.is_some());
        assert!(wrapper.last_seen >= before_update);

        // Mark it verified again
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1.clone(), true);

        let wrapper = &storage.get_all_verified()[0];

        // verify last_verified and last_seen updated again
        assert!(wrapper.last_verified.unwrap() >= before_update);
        assert!(wrapper.last_seen >= before_update);

        // Mark it seen (not verified)
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1, false);

        let wrapper = &storage.get_all_verified()[0];

        // It should still be verified but last_seen should be updated
        assert!(wrapper.last_verified.unwrap() <= before_update);
        assert!(wrapper.last_seen >= before_update);
    }

    #[test]
    fn test_get_nearest_nodes() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = NodeBucketStorage::new(our_id, 1);

        storage.add_or_update(
            Node::new(
                Id::from_hex("7fffffffffffffffffffffffffffffffffffffff").unwrap(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            ),
            true,
        );

        let closer_id = Id::from_hex("ffffffffffffffffffffffffffffffffffffffff").unwrap();
        storage.add_or_update(
            Node::new(
                closer_id,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            ),
            true,
        );

        let seeking_id = Id::from_hex("8000000000000000000000000000000000000000").unwrap();

        let result = storage.get_nearest_nodes(&seeking_id, None);
        assert_eq!(1, result.len());
        assert_eq!(closer_id, result[0].id);
    }

    #[test]
    fn test_empty_prune() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = NodeBucketStorage::new(our_id, 1);

        let period = Duration::from_secs(600);
        storage.prune(period, period);
    }

    #[test]
    fn test_clear() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = NodeBucketStorage::new(our_id, 1);

        storage.add_or_update(
            Node::new(
                Id::from_hex("7fffffffffffffffffffffffffffffffffffffff").unwrap(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            ),
            true,
        );
        assert_eq!(storage.count().1, 1);

        storage.clear();
        assert_eq!(storage.count().1, 0);
    }
}

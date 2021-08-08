use std::time::Instant;

use crate::common::{Id, Node};

use super::buckets::Buckets;
use super::node_wrapper::NodeWrapper;

pub trait NodeStorage {
    fn add_or_update(&mut self, node: Node, verified: bool);
    fn clear(&mut self);

    fn count_buckets(&self) -> usize;
    fn count(&self) -> (usize, usize);

    fn get_all_unverified(&self) -> Vec<&NodeWrapper>;
    fn get_all_verified(&self) -> Vec<&NodeWrapper>;
    fn get_nearest_nodes(&self, id: &Id, exclude: Option<&Id>) -> Vec<&Node>;

    /// Prunes verified to unverified. Prunes unverified to death.
    fn prune(&mut self, time: &Instant, unverified_time: &Instant);
    fn set_id(&mut self, our_id: Id);
}

pub struct NodeBucketStorage {
    verified: Buckets<NodeWrapper>,
    unverified: Buckets<NodeWrapper>,
}

impl NodeBucketStorage {
    pub fn new(our_id: Id, k: usize) -> NodeBucketStorage {
        NodeBucketStorage {
            verified: Buckets::new(our_id, k),
            unverified: Buckets::new(our_id, k),
        }
    }
    fn add_or_update_last_seen(&mut self, node: Node) {
        if let Some(existing) = self.verified.get_mut(&node.id) {
            eprintln!("Updating existing verified {:?} last seen", node);
            existing.last_seen = std::time::Instant::now();
        } else if let Some(existing) = self.unverified.get_mut(&node.id) {
            eprintln!("Updating existing unverified {:?} last seen", node);
            existing.last_seen = std::time::Instant::now();
        } else {
            eprintln!("Attempting to add unverified {:?}", node);
            self.unverified.add(NodeWrapper::new(node), None);
        }
    }

    fn add_or_update_verified(&mut self, node: Node) {
        let now = std::time::Instant::now();

        // Already exists in unverified.
        // Remove it and try to add it to Verified.
        // If verified is full, add whatever overflows back to unverified (if it fits)
        if let Some(mut item) = self.unverified.remove(&node.id) {
            eprintln!("Attempting to move {:?} from unverified to verified", node);
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
            eprintln!("Marking verified {:?} as verified again", node);
            wrapper.last_verified = Some(wrapper.last_seen);
            wrapper.last_seen = now;
            wrapper.last_verified = Some(now);
        }
        // Doesn't exist yet
        else {
            eprintln!("Marking new {:?} as verified", node);
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

    fn count_buckets(&self) -> usize {
        self.verified.count_buckets()
    }

    fn count(&self) -> (usize, usize) {
        (self.unverified.count(), self.verified.count())
    }

    fn get_all_unverified(&self) -> Vec<&NodeWrapper> {
        self.unverified.values()
    }

    fn get_all_verified(&self) -> Vec<&NodeWrapper> {
        self.verified.values()
    }

    fn get_nearest_nodes(&self, id: &Id, exclude: Option<&Id>) -> Vec<&Node> {
        self.verified
            .get_nearest_nodes(id, exclude)
            .iter()
            .map(|nw| &nw.node)
            .collect()
    }

    fn prune(&mut self, time: &Instant, unverified_time: &Instant) {
        self.verified.retain(|nw| {
            if let Some(last_verified) = nw.last_verified {
                return last_verified >= *time;
            }
            eprintln!("Verified {:?} hasn't verified recently. Removing.", nw.node);
            return false;
        });

        self.unverified.retain(|nw| {
            if let Some(last_verified) = nw.last_verified {
                if last_verified >= *time {
                    return true;
                }
            }

            if nw.last_seen >= *time && nw.last_seen >= *unverified_time {
                return true;
            }

            eprintln!("Unverified {:?} is dead. Removing", nw.node);
            return false;
        });
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
        let wrapper = storage.get_all_unverified()[0];

        // verify last_seen was updated, but still not verified
        assert!(wrapper.last_seen >= before_update);
        assert!(wrapper.last_verified.is_none());

        // Mark the node verified
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1.clone(), true);

        let wrapper = storage.get_all_verified()[0];

        // verify it's verified and last_seen updated again
        assert!(wrapper.last_verified.is_some());
        assert!(wrapper.last_seen >= before_update);

        // Mark it verified again
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1.clone(), true);

        let wrapper = storage.get_all_verified()[0];

        // verify last_verified and last_seen updated again
        assert!(wrapper.last_verified.unwrap() > before_update);
        assert!(wrapper.last_seen >= before_update);

        // Mark it seen (not verified)
        let before_update = std::time::Instant::now();
        storage.add_or_update(node1, false);

        let wrapper = storage.get_all_verified()[0];

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
}

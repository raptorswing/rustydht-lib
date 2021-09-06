use crate::common::Id;
use std::time::Instant;

pub trait Bucketable {
    fn get_id(&self) -> Id;

    fn get_first_seen(&self) -> Instant;
}

pub struct Buckets<T: Bucketable> {
    our_id: Id,
    buckets: Vec<Vec<T>>,

    k: usize,
}

impl<T: Bucketable> Buckets<T> {
    pub fn new(our_id: Id, k: usize) -> Buckets<T> {
        let mut to_ret = Buckets {
            our_id: our_id,
            buckets: Vec::with_capacity(32),
            k: k,
        };

        to_ret.buckets.push(Vec::new());

        to_ret
    }

    pub fn add(&mut self, item: T, chump_list: Option<&mut Vec<T>>) {
        // Never add our own node!
        if item.get_id() == self.our_id {
            return;
        }

        let dest_bucket_idx = self.get_dest_bucket_idx(&item);
        self.buckets[dest_bucket_idx].push(item);
        self.handle_bucket_overflow(dest_bucket_idx, chump_list);
    }

    pub fn clear(&mut self) {
        self.buckets.clear();
        self.buckets.push(Vec::with_capacity(2 * self.k));
    }

    pub fn count(&self) -> usize {
        let mut count = 0;
        for bucket in &self.buckets {
            count = count + bucket.len();
        }

        count
    }

    pub fn count_buckets(&self) -> usize {
        self.buckets.len()
    }

    pub fn get_mut(&mut self, id: &Id) -> Option<&mut T> {
        let dest_bucket_idx = self.get_dest_bucket_idx_for_id(&id);
        if let Some(bucket) = self.buckets.get_mut(dest_bucket_idx) {
            for item in bucket.iter_mut() {
                if item.get_id() == *id {
                    return Some(item);
                }
            }
        }
        None
    }

    pub fn get_nearest_nodes(&self, id: &Id, exclude: Option<&Id>) -> Vec<&T> {
        let mut all: Vec<&T> = self
            .values()
            .iter()
            .filter(|item| exclude.is_none() || *exclude.unwrap() != item.get_id())
            .map(|item| *item)
            .collect();

        all.sort_unstable_by(|a, b| {
            let a_dist = a.get_id().xor(id);
            let b_dist = b.get_id().xor(id);
            a_dist.partial_cmp(&b_dist).unwrap()
        });

        all.truncate(self.k);

        all
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        for bucket in &mut self.buckets {
            bucket.retain(|item| f(item));
        }
    }

    pub fn remove(&mut self, id: &Id) -> Option<T> {
        let dest_bucket_idx = self.get_dest_bucket_idx_for_id(&id);
        if let Some(bucket) = self.buckets.get_mut(dest_bucket_idx) {
            for i in 0..bucket.len() {
                if bucket[i].get_id() == *id {
                    return Some(bucket.swap_remove(i));
                }
            }
        }
        None
    }

    pub fn set_id(&mut self, new_id: Id) {
        self.clear();
        self.our_id = new_id;
    }

    pub fn values(&self) -> Vec<&T> {
        let mut to_ret = Vec::new();
        for bucket in &self.buckets {
            for item in bucket {
                to_ret.push(item);
            }
        }
        to_ret
    }

    fn get_dest_bucket_idx(&self, item: &T) -> usize {
        self.get_dest_bucket_idx_for_id(&item.get_id())
    }

    fn get_dest_bucket_idx_for_id(&self, id: &Id) -> usize {
        std::cmp::min(self.buckets.len() - 1, self.our_id.matching_prefix_bits(id))
    }

    fn handle_bucket_overflow(
        &mut self,
        mut bucket_index: usize,
        mut chump_list: Option<&mut Vec<T>>,
    ) {
        while bucket_index < self.buckets.len() {
            // Is the bucket over capacity?
            if self.buckets[bucket_index].len() > self.k {
                // Is this the "deepest" bucket?
                // If so, add a new one since we're over capacity
                if bucket_index == self.buckets.len() - 1 {
                    self.buckets.push(Vec::with_capacity(2 * self.k));
                }

                // (Hopefully) move some nodes out of this bucket into the next one
                for i in (0..self.buckets[bucket_index].len()).rev() {
                    let ideal_bucket_idx = self.get_dest_bucket_idx(&self.buckets[bucket_index][i]);

                    // This Node belongs in another bucket. Move it.
                    if ideal_bucket_idx != bucket_index {
                        let node = self.buckets[bucket_index].swap_remove(i);
                        self.buckets[ideal_bucket_idx].push(node);
                    }
                }

                // Sort by oldest. Move the newest excess to the chump list
                if self.buckets[bucket_index].len() > self.k {
                    self.buckets[bucket_index]
                        .sort_unstable_by(|a, b| a.get_first_seen().cmp(&b.get_first_seen()));
                    let mut remainder = self.buckets[bucket_index].split_off(self.k);

                    if let Some(chump_list) = &mut chump_list {
                        chump_list.append(&mut remainder);
                    }
                }
            }
            bucket_index = bucket_index + 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::prelude::*;
    extern crate rand_chacha;

    struct TestWrapper {
        id: Id,
        first_seen: Instant,
    }

    impl TestWrapper {
        pub fn new(id: Id, first_seen: Option<Instant>) -> TestWrapper {
            let fs = if let Some(first_seen) = first_seen {
                first_seen
            } else {
                Instant::now()
            };

            TestWrapper {
                id: id,
                first_seen: fs,
            }
        }
    }

    impl Bucketable for TestWrapper {
        fn get_id(&self) -> Id {
            self.id
        }

        fn get_first_seen(&self) -> Instant {
            self.first_seen
        }
    }

    impl std::fmt::Debug for TestWrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.get_id().fmt(f)
        }
    }

    /// Tests that items stay in the correct buckets as the number of buckets grows and that each bucket only contains the correct number of items
    #[test]
    fn test_correct_bucket() {
        let id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = Buckets::new(id, 8);

        // Create RNG with static seed to ensure tests are reproducible
        let mut rng = Box::new(rand_chacha::ChaCha8Rng::seed_from_u64(50));

        for _ in 0..2000 {
            let node_id = Id::from_random(&mut rng);
            storage.add(TestWrapper::new(node_id, None), None);
        }

        for i in 0..storage.buckets.len() {
            assert!(storage.buckets[i].len() <= 8);
            for wrapper in &storage.buckets[i] {
                assert_eq!(
                    i,
                    std::cmp::min(
                        storage.our_id.matching_prefix_bits(&wrapper.get_id()),
                        storage.buckets.len() - 1
                    )
                );
            }
        }
    }

    /// Tests that we can add and remove an item from the buckets.
    #[test]
    fn test_add_remove() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = Buckets::new(our_id, 8);

        let their_id = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
        storage.add(TestWrapper::new(their_id, None), None);

        assert!(storage.count() == 1);
        assert!(storage.get_mut(&their_id).is_some());

        assert!(storage.remove(&their_id).is_some());
        assert!(storage.remove(&their_id).is_none());
        assert!(storage.count() == 0);
    }

    /// Tests that we only store a max of k items that have nothing in common with our id
    #[test]
    fn test_nothing_in_common() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = Buckets::new(our_id, 8);

        // First 8 should be added
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000000").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000001").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000010").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000011").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000100").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000101").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000110").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000000111").unwrap(),
                None,
            ),
            None,
        );
        assert_eq!(storage.buckets[0].len(), 8);

        // This one should not be added
        storage.add(
            TestWrapper::new(
                Id::from_hex("f000000000000000000000000000000000001000").unwrap(),
                None,
            ),
            None,
        );
        assert_eq!(storage.buckets[0].len(), 8);
        assert!(storage
            .get_mut(&Id::from_hex("f000000000000000000000000000000000001000").unwrap())
            .is_none());
    }

    /// Test that get_nearest works
    #[test]
    fn test_get_nearest_nodes() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = Buckets::new(our_id, 8);

        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000001").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000010").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000011").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000100").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000101").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000110").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000000111").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000001000").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("0000000000000000000000000000000000001001").unwrap(),
                None,
            ),
            None,
        );

        let nearest = storage.get_nearest_nodes(
            &Id::from_hex("ffffffffffffffffffffffffffffffffffffffff").unwrap(),
            None,
        );
        assert_eq!(nearest.len(), 8);
        assert_eq!(
            nearest[0].get_id(),
            Id::from_hex("0000000000000000000000000000000000001001").unwrap()
        );

        let nearest = storage.get_nearest_nodes(
            &Id::from_hex("0000000000000000000000000000000000000000").unwrap(),
            None,
        );
        assert_eq!(nearest.len(), 8);
        assert_eq!(
            nearest[0].get_id(),
            Id::from_hex("0000000000000000000000000000000000000001").unwrap()
        );
    }

    #[test]
    fn test_get_nearest_nodes2() {
        let our_id = Id::from_hex("0000000000000000000000000000000000000000").unwrap();
        let mut storage = Buckets::new(our_id, 8);

        storage.add(
            TestWrapper::new(
                Id::from_hex("5fcb695a07ad50be46f100000000000000000000").unwrap(),
                None,
            ),
            None,
        );
        storage.add(
            TestWrapper::new(
                Id::from_hex("00000000000000000000fada4cd3cf6225373cb7").unwrap(),
                None,
            ),
            None,
        );

        let nearest = storage.get_nearest_nodes(
            &Id::from_hex("5fcb695a07ad50be46f1fada4cd3cf6225373cb7").unwrap(),
            None,
        );
        assert_eq!(nearest.len(), 2);
        assert_eq!(
            nearest[0].get_id(),
            Id::from_hex("5fcb695a07ad50be46f100000000000000000000").unwrap()
        );

        let nearest = storage.get_nearest_nodes(
            &Id::from_hex("0000000000000000000000000000000000000000").unwrap(),
            None,
        );
        assert_eq!(nearest.len(), 2);
        assert_eq!(
            nearest[0].get_id(),
            Id::from_hex("00000000000000000000fada4cd3cf6225373cb7").unwrap()
        );

        let nearest = storage.get_nearest_nodes(
            &Id::from_hex("ffffffffffffffffffffffffffffffffffffffff").unwrap(),
            None,
        );
        assert_eq!(nearest.len(), 2);
        assert_eq!(
            nearest[0].get_id(),
            Id::from_hex("5fcb695a07ad50be46f100000000000000000000").unwrap()
        );
    }
}

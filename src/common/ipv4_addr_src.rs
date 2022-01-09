use dyn_clone::DynClone;
use std::convert::TryInto;
use std::net::Ipv4Addr;

use log::debug;

/// Represents an object with methods for figuring out the DHT node's external IPv4 address
pub trait IPV4AddrSource: DynClone + Send {
    /// Retrieves the IPv4 address that the source thinks we should have,
    /// or None if it can't make a determination at this time.
    ///
    /// This method will be called periodically by the DHT. Implementations
    /// should return their current best guess for the external (globally routable) IPv4 address
    /// of the DHT.
    fn get_best_ipv4(&self) -> Option<Ipv4Addr>;

    /// Adds a "vote" from another node in the DHT in respose to our queries.
    ///
    /// DHT will call this method when it receive a "hint" from another DHT node
    /// about our external IPv4 address. An IPV4AddrSource implementation can
    /// use these "hints" or "votes", or ignore them.
    ///
    /// # Parameters
    /// * `their_addr` - The IP address of the DHT node that we're learning this information from.
    /// * `proposed_addr` - The external IP address that the other DHT node says we have.
    fn add_vote(&mut self, their_addr: Ipv4Addr, proposed_addr: Ipv4Addr);

    /// This will get called by DHT at some regular interval. Implementations
    /// can use it to allow old information to "decay" over time.
    fn decay(&mut self);
}

dyn_clone::clone_trait_object!(IPV4AddrSource);

/// An IPV4AddrSource that always returns the same thing
#[derive(Clone)]
pub struct StaticIPV4AddrSource {
    ip: Ipv4Addr,
}

impl IPV4AddrSource for StaticIPV4AddrSource {
    fn get_best_ipv4(&self) -> Option<Ipv4Addr> {
        Some(self.ip)
    }
    fn add_vote(&mut self, _: Ipv4Addr, _: Ipv4Addr) {}
    fn decay(&mut self) {}
}

impl StaticIPV4AddrSource {
    pub fn new(addr: Ipv4Addr) -> StaticIPV4AddrSource {
        StaticIPV4AddrSource { ip: addr }
    }
}

#[derive(Clone)]
struct IPV4Vote {
    ip: Ipv4Addr,
    votes: i32,
}

/// An IPV4Source that takes a certain number of "votes" from other nodes on the network to make its decision.
#[derive(Clone)]
pub struct IPV4Consensus {
    min_votes: usize,
    max_votes: usize,
    votes: Vec<IPV4Vote>,
}

impl IPV4Consensus {
    pub fn new(min_votes: usize, max_votes: usize) -> IPV4Consensus {
        IPV4Consensus {
            min_votes: min_votes,
            max_votes: max_votes,
            votes: Vec::new(),
        }
    }
}

impl IPV4AddrSource for IPV4Consensus {
    fn get_best_ipv4(&self) -> Option<Ipv4Addr> {
        let first = self.votes.first();
        match first {
            Some(vote_info) => {
                debug!(target: "rustydht_lib::IPV4AddrSource", "Best IPv4 address {:?} has {} votes", vote_info.ip, vote_info.votes);
                if vote_info.votes >= self.min_votes.try_into().unwrap() {
                    Some(vote_info.ip)
                } else {
                    None
                }
            }

            None => None,
        }
    }

    fn add_vote(&mut self, _: Ipv4Addr, proposed_addr: Ipv4Addr) {
        let mut do_sort = false;
        for vote in self.votes.iter_mut() {
            if vote.ip == proposed_addr {
                vote.votes = std::cmp::min(self.max_votes.try_into().unwrap(), vote.votes + 1);
                do_sort = true;
                break;
            }
        }

        if do_sort {
            self.votes.sort_by(|a, b| {
                return b.votes.cmp(&a.votes);
            });
        } else {
            self.votes.push(IPV4Vote {
                ip: proposed_addr,
                votes: 1,
            });
        }
    }

    fn decay(&mut self) {
        for vote in self.votes.iter_mut() {
            vote.votes = std::cmp::max(0, vote.votes - 1);
        }

        // Optimize this if we care (hint: we probably don't)
        self.votes.retain(|a| {
            return a.votes > 0;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_src() {
        let ip = Ipv4Addr::new(10, 0, 0, 107);
        let mut src = StaticIPV4AddrSource::new(ip);
        assert_eq!(Some(ip), src.get_best_ipv4());

        // Doesn't matter
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(1, 1, 1, 1));
        src.decay();

        // Always returns the same thing
        assert_eq!(Some(ip), src.get_best_ipv4());
    }

    #[test]
    fn test_consensus_src() {
        let mut src = IPV4Consensus::new(2, 4);
        // Nothing yet
        assert_eq!(None, src.get_best_ipv4());

        // One vote, but not enough
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(1, 1, 1, 1));
        assert_eq!(None, src.get_best_ipv4());

        // Competing vote, still nothing
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(2, 2, 2, 2));
        assert_eq!(None, src.get_best_ipv4());

        // Another vote for the first one. Got something now
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(1, 1, 1, 1));
        assert_eq!(Some(Ipv4Addr::new(1, 1, 1, 1)), src.get_best_ipv4());

        // Another vote for the second one. Should still return the first one because in this house our sorts are stable
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(2, 2, 2, 2));
        assert_eq!(Some(Ipv4Addr::new(1, 1, 1, 1)), src.get_best_ipv4());

        // Dark horse takes the lead
        src.add_vote(Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(2, 2, 2, 2));
        assert_eq!(Some(Ipv4Addr::new(2, 2, 2, 2)), src.get_best_ipv4());

        // Decay happens
        src.decay();

        // Dark horse still winning
        assert_eq!(Some(Ipv4Addr::new(2, 2, 2, 2)), src.get_best_ipv4());

        // Decay happens again
        src.decay();

        // Nobody wins now
        assert_eq!(None, src.get_best_ipv4());
    }
}

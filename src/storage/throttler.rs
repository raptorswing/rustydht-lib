use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use log::debug;

#[derive(Clone, Copy)]
struct ThrottlerRecord {
    ip: IpAddr,
    packets: usize,
    expiration: Instant,
    creation_time: Instant,
}

impl Default for ThrottlerRecord {
    fn default() -> Self {
        let now = Instant::now();
        ThrottlerRecord {
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            packets: 0,
            expiration: now,
            creation_time: now,
        }
    }
}

impl ThrottlerRecord {
    fn clear(&mut self) {
        let now = Instant::now();
        self.ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        self.packets = 0;
        self.expiration = now;
        self.creation_time = now;
    }
}

pub struct Throttler<const NUM_RECORDS: usize> {
    records: [ThrottlerRecord; NUM_RECORDS],

    rate_limit: usize,
    period: Duration,
    naughty_timeout: Duration,
    max_tracking: Duration,
}

impl<const NUM_RECORDS: usize> Throttler<NUM_RECORDS> {
    pub fn new(
        rate_limit: usize,
        period: Duration,
        naughty_timeout: Duration,
        max_tracking: Duration,
    ) -> Throttler<NUM_RECORDS> {
        Throttler {
            records: [ThrottlerRecord::default(); NUM_RECORDS],
            rate_limit: rate_limit,
            period: period,
            naughty_timeout: naughty_timeout,
            max_tracking: max_tracking,
        }
    }

    /// Returns true if the provided IP is throttled.
    pub fn check_throttle(&mut self, ip: IpAddr, now: Option<Instant>) -> bool {
        let now = match now {
            Some(instant) => instant,
            None => Instant::now(),
        };

        let mut found: Option<&mut ThrottlerRecord> = None;
        let mut lamest: Option<&mut ThrottlerRecord> = None;
        for record in &mut self.records {
            // If record exists for this IP, use it
            if record.ip == ip {
                found = Some(record);
                break;
            }

            // Keep track of the saddest/lamest record as we go
            if let Some(lame) = &lamest {
                if record.packets < lame.packets {
                    lamest = Some(record);
                } else if record.packets == lame.packets && record.expiration < lame.expiration {
                    lamest = Some(record);
                }
            } else {
                lamest = Some(record)
            }
        }

        if let Some(found) = found {
            // If this record has been around for longer than the max tracking time, don't use it and reset to blank
            if let Some(since_creation) = now.checked_duration_since(found.creation_time) {
                if since_creation > self.max_tracking {
                    found.clear();
                    return false;
                }
            }

            if now < found.expiration {
                found.packets = found.packets + 1;

                if found.packets > self.rate_limit {
                    debug!(target: "rustydht_lib::Throttler", "{} is throttled for {:?}. {} packets on record", ip, self.naughty_timeout, found.packets);
                    found.expiration = now + self.naughty_timeout;
                    return true;
                }
            } else {
                found.packets = 1;
                found.expiration = now + self.period;
            }
        } else if let Some(lamest) = lamest {
            lamest.packets = 1;
            lamest.expiration = now + self.period;
            lamest.ip = ip;
            lamest.creation_time = now;
        } else {
            panic!("This should never happen ;)");
        }

        false
    }

    pub fn get_num_records(&self) -> usize {
        return NUM_RECORDS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;
    use std::ops::Add;

    #[test]
    // Tests that throttling kicks in and expires with a single IP
    fn test_one_two_punch() {
        let mut throttler = Throttler::<32>::new(
            1,
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_secs(10),
        );
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 50, 1));

        assert!(!throttler.check_throttle(ip, None));
        assert!(throttler.check_throttle(ip, None));
        let fake_time = Instant::now().add(Duration::from_secs(2));
        assert!(!throttler.check_throttle(ip, Some(fake_time)));
    }

    #[test]
    // Tests that throttling kicks in and expires even if the bookkeeping is already 'full'
    fn test_lots_of_ips() {
        let mut throttler = Throttler::<32>::new(
            1,
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_secs(10),
        );

        let mut ip = IpAddr::V4(Ipv4Addr::new(192, 168, 50, 0));
        for a in 0..throttler.get_num_records() + 1 {
            let last_octet: u8 = a.try_into().unwrap();
            ip = IpAddr::V4(Ipv4Addr::new(192, 168, 50, last_octet));
            assert!(!throttler.check_throttle(ip, None));
        }
        assert!(throttler.check_throttle(ip, None));
        let fake_time = Instant::now().add(Duration::from_secs(2));
        assert!(!throttler.check_throttle(ip, Some(fake_time)));
    }

    #[test]
    // Tests that a record is reset after the max tracking period has been reached
    fn test_max_tracking() {
        let mut throttler = Throttler::<32>::new(
            1,
            Duration::from_secs(5),
            Duration::from_secs(100),
            Duration::from_secs(10),
        );
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 50, 1));

        assert!(!throttler.check_throttle(ip, None));
        assert!(throttler.check_throttle(ip, None));
        let fake_time = Instant::now().add(Duration::from_secs(9));
        assert!(throttler.check_throttle(ip, Some(fake_time)));

        let fake_time = Instant::now().add(Duration::from_secs(11));
        assert!(!throttler.check_throttle(ip, Some(fake_time)));
    }
}

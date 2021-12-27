//! A library for working with BitTorrent's "mainline" distributed hash table (DHT).
//!
//! ## Supported BEPs
//! - [x] [BEP0005 (DHT Protocol)](http://bittorrent.org/beps/bep_0005.html)
//! - [ ] [BEP0032 (IPv6 extension for DHT)](http://bittorrent.org/beps/bep_0032.html)
//! - [ ] [BEP0033 (DHT scrape)](http://bittorrent.org/beps/bep_0033.html)
//! - [x] [BEP0042 (DHT Security Extension)](http://bittorrent.org/beps/bep_0042.html)
//! - [x] [BEP0043 (Read-only DHT Nodes)](http://bittorrent.org/beps/bep_0043.html)
//! - [ ] [BEP0044 (Storing arbitrary data in the DHT)](http://bittorrent.org/beps/bep_0044.html)
//! - [ ] [BEP0045 (Multiple-address operation for the BitTorrent DHT)](http://bittorrent.org/beps/bep_0045.html)
//! - [ ] [BEP0046 (Updating Torrents Via DHT Mutable Items)](http://bittorrent.org/beps/bep_0046.html)
//! - [x] [BEP0051 (DHT Infohash Indexing)](http://bittorrent.org/beps/bep_0051.html)

/// Miscellaneous common structs used throughout the library
pub mod common;

/// Structs and functions with the core business logic of the DHT
pub mod dht;

/// library-specific error enums
pub mod errors;

/// Structs and functions for representing DHT messages and serializing and deserializing them
pub mod packets;

/// A utility for signaling that an async task should do a clean shutdown, and a utility listening for such signals
pub mod shutdown; // TODO move this into common?

/// Data structures used in the DHT - node buckets, peer storage, etc.
pub mod storage;

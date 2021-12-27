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

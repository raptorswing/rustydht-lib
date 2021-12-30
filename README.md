# rustydht-lib

[![Build Status](https://github.com/raptorswing/rustydht-lib/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/raptorswing/rustydht-lib/actions/workflows/rust.yml)
[![crates.io](https://img.shields.io/crates/v/rustydht-lib)](https://crates.io/crates/rustydht-lib)
[![License](https://img.shields.io/crates/l/rustydht-lib)](https://github.com/raptorswing/rustydht-lib/blob/v1.0.0/LICENSE)

A rust library for interacting with BitTorrent's mainline DHT. This is a work in progress and the public API is likely to change frequently.

**[API Docs](https://docs.rs/rustydht-lib/latest/rustydht_lib/)**

## Supported BEPs
- [x] [BEP0005 (DHT Protocol)](http://bittorrent.org/beps/bep_0005.html)
- [ ] [BEP0032 (IPv6 extension for DHT)](http://bittorrent.org/beps/bep_0032.html)
- [ ] [BEP0033 (DHT scrape)](http://bittorrent.org/beps/bep_0033.html)
- [x] [BEP0042 (DHT Security Extension)](http://bittorrent.org/beps/bep_0042.html)
- [x] [BEP0043 (Read-only DHT Nodes)](http://bittorrent.org/beps/bep_0043.html)
- [ ] [BEP0044 (Storing arbitrary data in the DHT)](http://bittorrent.org/beps/bep_0044.html)
- [ ] [BEP0045 (Multiple-address operation for the BitTorrent DHT)](http://bittorrent.org/beps/bep_0045.html)
- [ ] [BEP0046 (Updating Torrents Via DHT Mutable Items)](http://bittorrent.org/beps/bep_0046.html)
- [x] [BEP0051 (DHT Infohash Indexing)](http://bittorrent.org/beps/bep_0051.html)

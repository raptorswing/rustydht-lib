# rustydht-lib changelog

## [v2.1.0] - 2022-01-04
* Remove `timestamps` features from `simple_logger` in dev-dependencies. The time crate is intermittently failing to get local timezone offset and causing a crash while logging.
* Add `dht::operations` module with functions to announce_peer, find_node, and get_peers.
* Change `dht_node` example to accept a command line argument for its HTTP status server's listen IP/port. So you can change the default from 127.0.0.1 to 0.0.0.0 (or whatever) as desired.
* Fixed DHT behavior in read-only mode. It will no longer respond to requests in read-only mode.
* Fix `dht_node` example invocation in README.md
* Refactor common DHT request handling code into a common method.
* Change `dht_node` example's 'authors' and 'version' help metadata to be tied to the crate.

## [v2.0.1] - 2022-01-01
* Fix a Windows-only bug that can cause DHTSocket to error if someone sends it a datagram larger than the receive buffer.
* Fix a bug causing Message parsing to fail on get_peers responses with no nodes or peers

## [v2.0.0] - 2021-12-30
* Add MessageBuilder, a fluent interface for building Message structs. Remove the old create_ methods for creating Messages. This change makes breaking changes to the public API, and is the reason for the major version bump.
* Add an example called `dht_node` to the examples/ folder. It runs a DHT node and provides a simple HTTP status page.

[v2.1.0]: https://github.com/raptorswing/rustydht-lib/compare/v2.0.1...v2.1.0
[v2.0.1]: https://github.com/raptorswing/rustydht-lib/compare/v2.0.0...v2.0.1
[v2.0.0]: https://github.com/raptorswing/rustydht-lib/compare/v1.0.0...v2.0.0
# rustydht-lib changelog

## [v2.0.0] - 2021-12-30
* Add MessageBuilder, a fluent interface for building Message structs. Remove the old create_ methods for creating Messages. This change makes breaking changes to the public API, and is the reason for the major version bump.
* Add an example called `dht_node` to the examples/ folder. It runs a DHT node and provides a simple HTTP status page.

[v2.0.0]: https://github.com/raptorswing/rustydht-lib/compare/v1.0.0...v2.0.0
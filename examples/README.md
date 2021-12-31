# Examples

## dht_node
This example runs a DHT node and provides a small HTTP status page on localhost. The DHT and HTTP ports can be configured by command line arguments.

**To invoke the example with DHT port 6881 and HTTP status page on port 8080:**
```
cargo run --example dht_node -- -l 6881 -h 8080
```

## find_nodes
Command line tool that takes a target Id on the command line and searches the DHT for the nodes with the nearest Ids.

**Example:**
```
cargo run --example find_nodes -- -i ffffffffffffffffffffffffffffffffffffffff
```
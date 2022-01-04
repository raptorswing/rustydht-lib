# Examples

## announce_peer
Command line tool that takes a target info_hash and port, then announces to the DHT that your IP address is a peer for that torrent on the provided port.

**Example:**
```
cargo run --example announce_peer -- -i 1A70F7C9C882B5298F555AA7F4915E68A4F31C7E
```

## dht_node
This example runs a DHT node and provides a small HTTP status page on localhost. The DHT and HTTP ports can be configured by command line arguments.

**To invoke the example with DHT port 6881 and HTTP status page on port 8080:**
```
cargo run --example dht_node -- -l 6881 -h 0.0.0.0:8080
```

## find_nodes
Command line tool that takes a target Id on the command line and searches the DHT for the nodes with the nearest Ids.

**Example:**
```
cargo run --example find_nodes -- -i ffffffffffffffffffffffffffffffffffffffff
```

## get_peers
Command line tool that takes a torrent info_hash and searches the DHT for peers.

**Example:**
```
cargo run --example get_peers -- -i 6193AFC361B2896F2337E336BA9949B8EA8ACF5C
```
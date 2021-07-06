pub mod errors;

mod common;
pub use common::{Id, Node};

mod internal;
use internal::packets;

use anyhow::anyhow;

use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

const MAX_SCRAPE_INTERVAL: u64 = 21600; // 6 hours

#[derive(Debug, PartialEq)]
pub enum Message<const ID_SIZE: usize> {
    Request(RequestSpecific<ID_SIZE>),

    Response(ResponseSpecific<ID_SIZE>),

    Error(ErrorSpecific),
}

#[derive(Debug, PartialEq)]
pub enum RequestSpecific<const ID_SIZE: usize> {
    PingRequest(PingRequestArguments<ID_SIZE>),

    FindNodeRequest(FindNodeRequestArguments<ID_SIZE>),

    GetPeersRequest(GetPeersRequestArguments<ID_SIZE>),

    SampleInfoHashesRequest(SampleInfoHashesRequestArguments<ID_SIZE>),

    AnnouncePeerRequest(AnnouncePeerRequestArguments<ID_SIZE>),
}

#[derive(Debug, PartialEq)]
pub enum ResponseSpecific<const ID_SIZE: usize> {
    PingResponse(PingResponseArguments<ID_SIZE>),

    FindNodeResponse(FindNodeResponseArguments<ID_SIZE>),

    GetPeersResponse(GetPeersResponseArguments<ID_SIZE>),

    SampleInfoHashesResponse(SampleInfoHashesResponseArguments<ID_SIZE>),
    // AnnouncePeerResponse not needed - same as PingResponse
}

#[derive(Debug, PartialEq)]
pub struct PingRequestArguments<const ID_SIZE: usize> {
    requester_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    requester_version: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub struct FindNodeRequestArguments<const ID_SIZE: usize> {
    target: Id<ID_SIZE>,
    requester_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    requester_version: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub struct GetPeersRequestArguments<const ID_SIZE: usize> {
    info_hash: Id<ID_SIZE>,
    requester_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    requester_version: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub struct SampleInfoHashesRequestArguments<const ID_SIZE: usize> {
    target: Id<ID_SIZE>,
    requester_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    requester_version: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub struct AnnouncePeerRequestArguments<const ID_SIZE: usize> {
    requester_id: Id<ID_SIZE>,
    info_hash: Id<ID_SIZE>,
    port: u16,
    implied_port: Option<bool>,
    token: Vec<u8>,
    transaction_id: Vec<u8>,
    requester_version: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub enum GetPeersResponseValues<const ID_SIZE: usize> {
    Nodes(Vec<Node<ID_SIZE>>),
    Peers(Vec<SocketAddr>),
}

#[derive(Debug, PartialEq)]
pub struct PingResponseArguments<const ID_SIZE: usize> {
    responder_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    responder_version: Option<Vec<u8>>,
    requester_ip: Option<SocketAddr>,
}

#[derive(Debug, PartialEq)]
pub struct FindNodeResponseArguments<const ID_SIZE: usize> {
    responder_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    responder_version: Option<Vec<u8>>,
    requester_ip: Option<SocketAddr>,
    nodes: Vec<Node<ID_SIZE>>,
}

#[derive(Debug, PartialEq)]
pub struct GetPeersResponseArguments<const ID_SIZE: usize> {
    responder_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    responder_version: Option<Vec<u8>>,
    requester_ip: Option<SocketAddr>,
    token: Vec<u8>,
    values: GetPeersResponseValues<ID_SIZE>,
}

#[derive(Debug, PartialEq)]
pub struct SampleInfoHashesResponseArguments<const ID_SIZE: usize> {
    responder_id: Id<ID_SIZE>,
    transaction_id: Vec<u8>,
    responder_version: Option<Vec<u8>>,
    requester_ip: Option<SocketAddr>,
    interval: std::time::Duration,
    nodes: Vec<Node<ID_SIZE>>,
    samples: Vec<Id<ID_SIZE>>,
    num: i32,
}

#[derive(Debug, PartialEq)]
pub enum ErrorSpecific {}

impl<const ID_SIZE: usize> Message<ID_SIZE> {
    fn to_serde_message(&self) -> packets::DHTMessage {
        match &self {
            Message::Request(req) => match req {
                RequestSpecific::PingRequest(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.requester_version.clone(),
                    ip: None,
                    variant: packets::DHTMessageVariant::DHTRequest(
                        packets::DHTRequestSpecific::DHTPingRequest {
                            arguments: packets::DHTPingArguments {
                                id: arguments.requester_id.to_vec(),
                            },
                        },
                    ),
                },

                RequestSpecific::FindNodeRequest(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.requester_version.clone(),
                    ip: None,
                    variant: packets::DHTMessageVariant::DHTRequest(
                        packets::DHTRequestSpecific::DHTFindNodeRequest {
                            arguments: packets::DHTFindNodeArguments {
                                id: arguments.requester_id.to_vec(),
                                target: arguments.target.to_vec(),
                            },
                        },
                    ),
                },

                RequestSpecific::GetPeersRequest(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.requester_version.clone(),
                    ip: None,
                    variant: packets::DHTMessageVariant::DHTRequest(
                        packets::DHTRequestSpecific::DHTGetPeersRequest {
                            arguments: packets::DHTGetPeersArguments {
                                id: arguments.requester_id.to_vec(),
                                info_hash: arguments.info_hash.to_vec(),
                            },
                        },
                    ),
                },

                RequestSpecific::SampleInfoHashesRequest(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.requester_version.clone(),
                    ip: None,
                    variant: packets::DHTMessageVariant::DHTRequest(
                        packets::DHTRequestSpecific::DHTSampleInfoHashesRequest {
                            arguments: packets::DHTSampleInfoHashesRequestArguments {
                                id: arguments.requester_id.to_vec(),
                                target: arguments.target.to_vec(),
                            },
                        },
                    ),
                },

                RequestSpecific::AnnouncePeerRequest(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.requester_version.clone(),
                    ip: None,
                    variant: packets::DHTMessageVariant::DHTRequest(
                        packets::DHTRequestSpecific::DHTAnnouncePeerRequest {
                            arguments: packets::DHTAnnouncePeerRequestArguments {
                                id: arguments.requester_id.to_vec(),
                                implied_port: if arguments.implied_port.is_none() {
                                    None
                                } else if arguments.implied_port.unwrap() {
                                    Some(1)
                                } else {
                                    Some(0)
                                },
                                info_hash: arguments.info_hash.to_vec(),
                                port: arguments.port,
                                token: arguments.token.clone(),
                            },
                        },
                    ),
                },
            },

            Message::Response(res) => match res {
                ResponseSpecific::FindNodeResponse(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.responder_version.clone(),
                    ip: match arguments.requester_ip {
                        None => None,
                        Some(sockaddr) => Some(sockaddr_to_bytes(&sockaddr)),
                    },
                    variant: packets::DHTMessageVariant::DHTResponse(
                        packets::DHTResponseSpecific::DHTFindNodeResponse {
                            arguments: packets::DHTFindNodeResponseArguments {
                                id: arguments.responder_id.to_vec(),
                                nodes: nodes4_to_bytes(&arguments.nodes),
                            },
                        },
                    ),
                },

                ResponseSpecific::GetPeersResponse(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.responder_version.clone(),
                    ip: match arguments.requester_ip {
                        None => None,
                        Some(sockaddr) => Some(sockaddr_to_bytes(&sockaddr)),
                    },
                    variant: packets::DHTMessageVariant::DHTResponse(
                        packets::DHTResponseSpecific::DHTGetPeersResponse {
                            arguments: packets::DHTGetPeersResponseArguments {
                                id: arguments.responder_id.to_vec(),
                                token: arguments.token.clone(),
                                nodes: match &arguments.values {
                                    GetPeersResponseValues::Nodes(nodes) => {
                                        Some(nodes4_to_bytes(&nodes))
                                    }
                                    _ => None,
                                },
                                values: match &arguments.values {
                                    GetPeersResponseValues::Peers(peers) => {
                                        Some(peers_to_bytes(peers))
                                    }
                                    _ => None,
                                },
                            },
                        },
                    ),
                },

                ResponseSpecific::PingResponse(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.responder_version.clone(),
                    ip: match arguments.requester_ip {
                        None => None,
                        Some(sockaddr) => Some(sockaddr_to_bytes(&sockaddr)),
                    },
                    variant: packets::DHTMessageVariant::DHTResponse(
                        packets::DHTResponseSpecific::DHTPingResponse {
                            arguments: packets::DHTPingResponseArguments {
                                id: arguments.responder_id.to_vec(),
                            },
                        },
                    ),
                },

                ResponseSpecific::SampleInfoHashesResponse(arguments) => packets::DHTMessage {
                    transaction_id: arguments.transaction_id.clone(),
                    version: arguments.responder_version.clone(),
                    ip: match arguments.requester_ip {
                        None => None,
                        Some(sockaddr) => Some(sockaddr_to_bytes(&sockaddr)),
                    },
                    variant: packets::DHTMessageVariant::DHTResponse(
                        packets::DHTResponseSpecific::DHTSampleInfoHashesResponse {
                            arguments: packets::DHTSampleInfoHashesResponseArguments {
                                id: arguments.responder_id.to_vec(),
                                interval: std::cmp::min(
                                    MAX_SCRAPE_INTERVAL,
                                    arguments.interval.as_secs(),
                                ) as i32,
                                num: arguments.num,
                                nodes: nodes4_to_bytes(&arguments.nodes),
                                samples: {
                                    let mut a =
                                        Vec::with_capacity(arguments.samples.len() * ID_SIZE);
                                    for info_hash in &arguments.samples {
                                        a.append(&mut info_hash.to_vec());
                                    }
                                    a
                                },
                            },
                        },
                    ),
                }
            },

            // Message::Error(err) => {

            // }
            _ => {
                panic!("Message type not implemented!");
            }
        }
    }

    fn from_serde_message(
        msg: &packets::DHTMessage,
    ) -> Result<Message<ID_SIZE>, errors::RustyDHTError> {
        match &msg.variant {
            packets::DHTMessageVariant::DHTRequest(request_variant) => match request_variant {
                packets::DHTRequestSpecific::DHTPingRequest { arguments } => Ok(Message::Request(
                    RequestSpecific::PingRequest(PingRequestArguments {
                        transaction_id: msg.transaction_id.clone(),
                        requester_version: msg.version.clone(),
                        requester_id: Id::from_bytes(&arguments.id)?,
                    }),
                )),

                packets::DHTRequestSpecific::DHTAnnouncePeerRequest { arguments } => {
                    Ok(Message::Request(RequestSpecific::AnnouncePeerRequest(
                        AnnouncePeerRequestArguments {
                            transaction_id: msg.transaction_id.clone(),
                            requester_version: msg.version.clone(),
                            requester_id: Id::from_bytes(&arguments.id)?,
                            implied_port: if arguments.implied_port.is_none() {
                                None
                            } else if arguments.implied_port.unwrap() != 0 {
                                Some(true)
                            } else {
                                Some(false)
                            },
                            info_hash: Id::from_bytes(&arguments.info_hash)?,
                            port: arguments.port,
                            token: arguments.token.clone(),
                        },
                    )))
                }

                packets::DHTRequestSpecific::DHTFindNodeRequest { arguments } => Ok(
                    Message::Request(RequestSpecific::FindNodeRequest(FindNodeRequestArguments {
                        transaction_id: msg.transaction_id.clone(),
                        requester_version: msg.version.clone(),
                        requester_id: Id::from_bytes(&arguments.id)?,
                        target: Id::from_bytes(&arguments.target)?,
                    })),
                ),

                packets::DHTRequestSpecific::DHTGetPeersRequest { arguments } => Ok(
                    Message::Request(RequestSpecific::GetPeersRequest(GetPeersRequestArguments {
                        transaction_id: msg.transaction_id.clone(),
                        requester_version: msg.version.clone(),
                        requester_id: Id::from_bytes(&arguments.id)?,
                        info_hash: Id::from_bytes(&arguments.info_hash)?,
                    })),
                ),

                packets::DHTRequestSpecific::DHTSampleInfoHashesRequest { arguments } => {
                    Ok(Message::Request(RequestSpecific::SampleInfoHashesRequest(
                        SampleInfoHashesRequestArguments {
                            transaction_id: msg.transaction_id.clone(),
                            requester_version: msg.version.clone(),
                            requester_id: Id::from_bytes(&arguments.id)?,
                            target: Id::from_bytes(&arguments.target)?,
                        },
                    )))
                }
            },

            packets::DHTMessageVariant::DHTResponse(response_variant) => match response_variant {
                packets::DHTResponseSpecific::DHTFindNodeResponse { arguments } => {
                    Ok(Message::Response(ResponseSpecific::FindNodeResponse(
                        FindNodeResponseArguments {
                            transaction_id: msg.transaction_id.clone(),
                            responder_id: Id::from_bytes(&arguments.id)?,
                            responder_version: msg.version.clone(),
                            requester_ip: if msg.ip.is_none() {
                                None
                            } else {
                                Some(bytes_to_sockaddr(&msg.ip.as_ref().unwrap())?)
                            },
                            nodes: bytes_to_nodes4(&arguments.nodes)?,
                        },
                    )))
                }

                packets::DHTResponseSpecific::DHTGetPeersResponse { arguments } => {
                    if arguments.values.is_none() && arguments.nodes.is_none() {
                        return Err(anyhow!("Either values or nodes must be Some").into());
                    }

                    Ok(Message::Response(ResponseSpecific::GetPeersResponse(
                        GetPeersResponseArguments {
                            transaction_id: msg.transaction_id.clone(),
                            responder_id: Id::from_bytes(&arguments.id)?,
                            responder_version: msg.version.clone(),
                            requester_ip: match &msg.ip {
                                Some(ip) => Some(bytes_to_sockaddr(ip)?),
                                _ => None,
                            },
                            token: arguments.token.clone(),
                            values: if arguments.values.is_some() {
                                GetPeersResponseValues::Peers(bytes_to_peers(
                                    &arguments.values.as_ref().unwrap(),
                                )?)
                            } else {
                                GetPeersResponseValues::Nodes(bytes_to_nodes4(
                                    &arguments.nodes.as_ref().unwrap(),
                                )?)
                            },
                        },
                    )))
                }

                packets::DHTResponseSpecific::DHTPingResponse { arguments } => Ok(
                    Message::Response(ResponseSpecific::PingResponse(PingResponseArguments {
                        transaction_id: msg.transaction_id.clone(),
                        responder_id: Id::from_bytes(&arguments.id)?,
                        responder_version: msg.version.clone(),
                        requester_ip: match &msg.ip {
                            Some(ip) => Some(bytes_to_sockaddr(ip)?),
                            _ => None,
                        },
                    })),
                ),

                packets::DHTResponseSpecific::DHTSampleInfoHashesResponse { arguments } => {
                    if arguments.interval < 0 || arguments.interval > MAX_SCRAPE_INTERVAL as i32 {
                        return Err(anyhow!("interval is out of range. Received {} but should be 0 <= interval <= {}", arguments.interval, MAX_SCRAPE_INTERVAL).into());
                    }
                    Ok(Message::Response(
                        ResponseSpecific::SampleInfoHashesResponse(
                            SampleInfoHashesResponseArguments {
                                transaction_id: msg.transaction_id.clone(),
                                responder_id: Id::from_bytes(&arguments.id)?,
                                responder_version: msg.version.clone(),
                                requester_ip: match &msg.ip {
                                    Some(ip) => Some(bytes_to_sockaddr(ip)?),
                                    _ => None,
                                },
                                interval: std::time::Duration::from_secs(arguments.interval as u64),
                                num: arguments.num,
                                nodes: bytes_to_nodes4(&arguments.nodes)?,
                                samples: {
                                    if arguments.samples.len() % ID_SIZE != 0 {
                                        return Err(anyhow!(
                                            "Wrong sample length {} not a multiple of {}",
                                            arguments.samples.len(),
                                            ID_SIZE
                                        ).into());
                                    }
                                    let num_expected = arguments.samples.len() / ID_SIZE;
                                    let mut to_ret = Vec::with_capacity(num_expected);

                                    for i in 0..num_expected {
                                        let i = i * ID_SIZE;
                                        let id = Id::<ID_SIZE>::from_bytes(&arguments.samples[i..i+ID_SIZE])?;
                                        to_ret.push(id);
                                    }

                                    to_ret
                                },
                            },
                        ),
                    ))
                }
            },

            _ => {
                panic!("Not implemented");
            }
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, errors::RustyDHTError> {
        self.to_serde_message().to_bytes()
    }

    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Message<ID_SIZE>, errors::RustyDHTError> {
        Message::from_serde_message(&packets::DHTMessage::from_bytes(bytes)?)
    }
}

fn bytes_to_sockaddr<T: AsRef<[u8]>>(bytes: T) -> Result<SocketAddr, errors::RustyDHTError> {
    let bytes = bytes.as_ref();
    match bytes.len() {
        6 => {
            let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);

            let port_bytes_as_array: [u8; 2] =
                bytes[4..6]
                    .try_into()
                    .map_err(|err: std::array::TryFromSliceError| {
                        errors::RustyDHTError::PacketParseError(err.into())
                    })?;

            let port: u16 = u16::from_be_bytes(port_bytes_as_array);

            Ok(SocketAddr::new(IpAddr::V4(ip), port))
        }

        18 => Err(errors::RustyDHTError::PacketParseError(anyhow!(
            "IPv6 is not yet implemented"
        ))),

        _ => Err(errors::RustyDHTError::PacketParseError(anyhow!(
            "Wrong number of bytes for sockaddr"
        ))),
    }
}

fn sockaddr_to_bytes(sockaddr: &SocketAddr) -> Vec<u8> {
    let mut to_ret = Vec::new();

    match sockaddr {
        SocketAddr::V4(v4) => {
            let ip_bytes = v4.ip().octets();
            for i in 0..ip_bytes.len() {
                to_ret.push(ip_bytes[i]);
            }
        }

        SocketAddr::V6(v6) => {
            let ip_bytes = v6.ip().octets();
            for i in 0..ip_bytes.len() {
                to_ret.push(ip_bytes[i]);
            }
        }
    }

    let port_bytes = sockaddr.port().to_be_bytes();
    to_ret.push(port_bytes[0]);
    to_ret.push(port_bytes[1]);

    return to_ret;
}

fn bytes_to_nodes4<T: AsRef<[u8]>, const ID_SIZE: usize>(
    bytes: T,
) -> Result<Vec<Node<ID_SIZE>>, errors::RustyDHTError> {
    let bytes = bytes.as_ref();
    let node4_byte_size: usize = ID_SIZE + 6;
    if bytes.len() % node4_byte_size != 0 {
        return Err(anyhow!("Wrong number of bytes for nodes message ({})", bytes.len()).into());
    }

    let expected_num = bytes.len() / node4_byte_size;
    let mut to_ret = Vec::with_capacity(expected_num);
    for i in 0..bytes.len() / node4_byte_size {
        let i = i * node4_byte_size;
        let id = Id::from_bytes(&bytes[i..i + ID_SIZE])?;
        let sockaddr = bytes_to_sockaddr(&bytes[i + ID_SIZE..i + node4_byte_size])?;
        let node = Node::new(id, sockaddr);
        to_ret.push(node);
    }

    Ok(to_ret)
}

fn nodes4_to_bytes<const ID_SIZE: usize>(nodes: &Vec<Node<ID_SIZE>>) -> Vec<u8> {
    let node4_byte_size: usize = ID_SIZE + 6;
    let mut to_ret = Vec::with_capacity(node4_byte_size * nodes.len());
    for node in nodes {
        to_ret.append(&mut node.id.to_vec());
        to_ret.append(&mut sockaddr_to_bytes(&node.address));
    }
    to_ret
}

fn peers_to_bytes<T: AsRef<[SocketAddr]>>(peers: T) -> Vec<serde_bytes::ByteBuf> {
    let peers = peers.as_ref();
    peers
        .iter()
        .map(|p| serde_bytes::ByteBuf::from(sockaddr_to_bytes(p)))
        .collect()
}

fn bytes_to_peers<T: AsRef<[serde_bytes::ByteBuf]>>(
    bytes: T,
) -> Result<Vec<SocketAddr>, errors::RustyDHTError> {
    let bytes = bytes.as_ref();
    bytes.iter().map(|p| bytes_to_sockaddr(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_request() {
        let original_msg = Message::Request(RequestSpecific::PingRequest(PingRequestArguments {
            requester_id: Id::<2>::from_hex("f00f").unwrap(),
            requester_version: None,
            transaction_id: vec![0, 1, 2],
        }));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_ping_response() {
        let original_msg =
            Message::Response(ResponseSpecific::PingResponse(PingResponseArguments {
                responder_id: Id::<2>::from_hex("beef").unwrap(),
                transaction_id: vec![1, 2, 3],
                responder_version: Some(vec![0xde, 0xad]),
                requester_ip: Some("99.100.101.102:1030".parse().unwrap()),
            }));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_get_peers_request() {
        let original_msg =
            Message::Request(RequestSpecific::GetPeersRequest(GetPeersRequestArguments {
                info_hash: Id::<2>::from_hex("dead").unwrap(),
                requester_id: Id::<2>::from_hex("beef").unwrap(),
                transaction_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                requester_version: Some(vec![72, 73]),
            }));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_get_peers_response() {
        let original_msg = Message::Response(ResponseSpecific::GetPeersResponse(
            GetPeersResponseArguments {
                responder_id: Id::<2>::from_hex("0505").unwrap(),
                transaction_id: vec![1, 2, 3],
                responder_version: Some(vec![1]),
                requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
                token: vec![99, 100, 101, 102],
                values: GetPeersResponseValues::Nodes(vec![Node::new(
                    Id::<2>::from_hex("0606").unwrap(),
                    "49.50.52.52:5354".parse().unwrap(),
                )]),
            },
        ));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_find_node_request() {
        let original_msg =
            Message::Request(RequestSpecific::FindNodeRequest(FindNodeRequestArguments {
                target: Id::<2>::from_hex("1234").unwrap(),
                requester_id: Id::<2>::from_hex("5678").unwrap(),
                transaction_id: vec![1, 2, 3],
                requester_version: Some(vec![0x62, 0x61, 0x72, 0x66]),
            }));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_find_node_response() {
        let original_msg = Message::Response(ResponseSpecific::FindNodeResponse(
            FindNodeResponseArguments {
                responder_id: Id::<2>::from_hex("0505").unwrap(),
                transaction_id: vec![1, 2, 3],
                responder_version: Some(vec![1]),
                requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
                nodes: vec![Node::new(
                    Id::<2>::from_hex("0606").unwrap(),
                    "49.50.52.52:5354".parse().unwrap(),
                )],
            },
        ));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_announce_peer_request() {
        let original_msg = Message::Request(RequestSpecific::AnnouncePeerRequest(
            AnnouncePeerRequestArguments {
                requester_id: Id::<2>::from_hex("5678").unwrap(),
                transaction_id: vec![1, 2, 3],
                requester_version: Some(vec![0x62, 0x61, 0x72, 0x66]),
                port: 666,
                implied_port: Some(false),
                token: vec![42, 42, 42, 42],
                info_hash: Id::<2>::from_hex("9899").unwrap(),
            },
        ));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_sample_info_hashes_request() {
        let original_msg = Message::Request(RequestSpecific::SampleInfoHashesRequest(
            SampleInfoHashesRequestArguments {
                requester_id: Id::<2>::from_hex("5678").unwrap(),
                transaction_id: vec![1, 2, 3],
                requester_version: Some(vec![0x62, 0x61, 0x72, 0x66]),
                target: Id::<2>::from_hex("3344").unwrap(),
            },
        ));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_sample_info_hashes_response() {
        let original_msg = Message::Response(ResponseSpecific::SampleInfoHashesResponse(
            SampleInfoHashesResponseArguments {
                responder_id: Id::<2>::from_hex("0505").unwrap(),
                transaction_id: vec![1, 2, 3],
                responder_version: Some(vec![1]),
                requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
                interval: std::time::Duration::from_secs(32),
                nodes: vec![Node::new(
                    Id::<2>::from_hex("0606").unwrap(),
                    "49.50.52.52:5354".parse().unwrap(),
                )],
                samples: vec![
                    Id::<2>::from_hex("3232").unwrap(),
                    Id::<2>::from_hex("3434").unwrap(),
                ],
                num: 300,
            },
        ));

        let serde_msg = original_msg.to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = packets::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::<2>::from_serde_message(&parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }
}

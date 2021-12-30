use super::internal;
use crate::common::{Id, Node, ID_SIZE};
use crate::errors;
use anyhow::anyhow;
use log::warn;
use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

const MAX_SCRAPE_INTERVAL: u64 = 21600; // 6 hours

/// All packets sent and received via DHT are a serialized version of this struct.
///
/// It can be used to represent DHT messages throughout a program and has methods to
/// [serialize](crate::packets::Message::to_bytes) and [deserialize](crate::packets::Message::from_bytes) DHT messages.
///
/// # Building
/// The easiest way to build Message structs is to use [MessageBuilder](crate::packets::MessageBuilder).
///
/// But if you need more control/flexibility than provided by MessageBuilder, you can build Message structs
/// directly:
///
/// ```
/// use rustydht_lib::common::Id;
/// use rustydht_lib::packets::{Message, MessageType, RequestSpecific, FindNodeRequestArguments};
///
/// // This constructs a find_node request. It would be easier with MessageBuilder.
/// let msg = Message {
///     transaction_id: vec![1, 2, 3],
///     version: Some(vec![0x62, 0x61, 0x72, 0x66]),
///     requester_ip: None,
///     read_only: None,
///     message_type: MessageType::Request(RequestSpecific::FindNodeRequest(
///         FindNodeRequestArguments {
///             target: Id::from_hex("1234123412341234123412341234123412341234").unwrap(),
///             requester_id: Id::from_hex("5678567856785678567856785678567856785678").unwrap(),
///         },
///     )),
/// };
/// ```
///
/// # Deserializing
/// ```
/// use rustydht_lib::packets::Message;
///
/// // Imagine that this vector contains bytes from reading from a socket
/// let bytes: Vec<u8> = Vec::new();
/// match Message::from_bytes(&bytes) {
///     Ok(msg) => {
///         // Success! do something with the Message you just parsed
///     }
///     Err(e) => {
///         eprintln!("Oh no! I hit an error while parsing a Message: {}", e);
///     }
/// }
/// ```
///
/// # Serializing
/// ```
/// use rustydht_lib::common::Id;
/// use rustydht_lib::packets::{Message, MessageBuilder};
///
/// let our_id = Id::from_hex("0000000000000000000000000000000000000001").unwrap();
/// let target = Id::from_hex("ff00000000000000000000000000000000000002").unwrap();
/// let msg = MessageBuilder::new_find_node_request()
///     .sender_id(our_id)
///     .target(target)
///     .build()
///     .unwrap();
/// match msg.to_bytes() {
///     Ok(bytes) => {
///         // Success! You have a Vec<u8> that can be sent over a socket or whatever
///     }
///     Err(e) => {
///         eprintln!("Oh no! Couldn't serialize: {}", e);
///     }
/// }
/// ```
#[derive(Debug, PartialEq, Clone)]
pub struct Message {
    pub transaction_id: Vec<u8>,

    /// The version of the requester or responder.
    pub version: Option<Vec<u8>>,

    /// The IP address and port ("SocketAddr") of the requester as seen from the responder's point of view.
    /// This should be set only on response, but is defined at this level with the other common fields to avoid defining yet another layer on the response objects.
    pub requester_ip: Option<SocketAddr>,

    pub message_type: MessageType,

    /// For bep0043. When set true on a request, indicates that the requester can't reply to requests and that responders should not add requester to their routing tables.
    /// Should only be set on requests - undefined behavior when set on a response.
    pub read_only: Option<bool>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum MessageType {
    Request(RequestSpecific),

    Response(ResponseSpecific),

    Error(ErrorSpecific),
}

#[derive(Debug, PartialEq, Clone)]
pub enum RequestSpecific {
    PingRequest(PingRequestArguments),

    FindNodeRequest(FindNodeRequestArguments),

    GetPeersRequest(GetPeersRequestArguments),

    SampleInfoHashesRequest(SampleInfoHashesRequestArguments),

    AnnouncePeerRequest(AnnouncePeerRequestArguments),
}

#[derive(Debug, PartialEq, Clone)]
pub enum ResponseSpecific {
    PingResponse(PingResponseArguments),

    FindNodeResponse(FindNodeResponseArguments),

    GetPeersResponse(GetPeersResponseArguments),

    SampleInfoHashesResponse(SampleInfoHashesResponseArguments),
    // AnnouncePeerResponse not needed - same as PingResponse
}

#[derive(Debug, PartialEq, Clone)]
pub struct PingRequestArguments {
    pub requester_id: Id,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FindNodeRequestArguments {
    pub target: Id,
    pub requester_id: Id,
}

#[derive(Debug, PartialEq, Clone)]
pub struct GetPeersRequestArguments {
    pub info_hash: Id,
    pub requester_id: Id,
}

#[derive(Debug, PartialEq, Clone)]
pub struct SampleInfoHashesRequestArguments {
    pub target: Id,
    pub requester_id: Id,
}

#[derive(Debug, PartialEq, Clone)]
pub struct AnnouncePeerRequestArguments {
    pub requester_id: Id,
    pub info_hash: Id,
    pub port: u16,
    pub implied_port: Option<bool>,
    pub token: Vec<u8>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum GetPeersResponseValues {
    Nodes(Vec<Node>),
    Peers(Vec<SocketAddr>),
}

#[derive(Debug, PartialEq, Clone)]
pub struct PingResponseArguments {
    pub responder_id: Id,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FindNodeResponseArguments {
    pub responder_id: Id,
    pub nodes: Vec<Node>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct GetPeersResponseArguments {
    pub responder_id: Id,
    pub token: Vec<u8>,
    pub values: GetPeersResponseValues,
}

#[derive(Debug, PartialEq, Clone)]
pub struct SampleInfoHashesResponseArguments {
    pub responder_id: Id,
    pub interval: Duration,
    pub nodes: Vec<Node>,
    pub samples: Vec<Id>,
    pub num: i32,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ErrorSpecific {
    pub code: i32,
    pub description: String,
}

impl Message {
    fn to_serde_message(self) -> internal::DHTMessage {
        internal::DHTMessage {
            transaction_id: self.transaction_id,
            version: self.version,
            ip: match self.requester_ip {
                None => None,
                Some(sockaddr) => Some(sockaddr_to_bytes(&sockaddr)),
            },
            read_only: match self.read_only {
                None => None,
                Some(read_only) => Some(if read_only { 1 } else { 0 }),
            },
            variant: match self.message_type {
                MessageType::Request(req) => internal::DHTMessageVariant::DHTRequest(match req {
                    RequestSpecific::PingRequest(ping_args) => {
                        internal::DHTRequestSpecific::DHTPingRequest {
                            arguments: internal::DHTPingArguments {
                                id: ping_args.requester_id.to_vec(),
                            },
                        }
                    }

                    RequestSpecific::FindNodeRequest(find_node_args) => {
                        internal::DHTRequestSpecific::DHTFindNodeRequest {
                            arguments: internal::DHTFindNodeArguments {
                                id: find_node_args.requester_id.to_vec(),
                                target: find_node_args.target.to_vec(),
                            },
                        }
                    }

                    RequestSpecific::GetPeersRequest(get_peers_args) => {
                        internal::DHTRequestSpecific::DHTGetPeersRequest {
                            arguments: internal::DHTGetPeersArguments {
                                id: get_peers_args.requester_id.to_vec(),
                                info_hash: get_peers_args.info_hash.to_vec(),
                            },
                        }
                    }

                    RequestSpecific::SampleInfoHashesRequest(sample_info_hashes_args) => {
                        internal::DHTRequestSpecific::DHTSampleInfoHashesRequest {
                            arguments: internal::DHTSampleInfoHashesRequestArguments {
                                id: sample_info_hashes_args.requester_id.to_vec(),
                                target: sample_info_hashes_args.target.to_vec(),
                            },
                        }
                    }

                    RequestSpecific::AnnouncePeerRequest(announce_peer_args) => {
                        internal::DHTRequestSpecific::DHTAnnouncePeerRequest {
                            arguments: internal::DHTAnnouncePeerRequestArguments {
                                id: announce_peer_args.requester_id.to_vec(),
                                implied_port: if announce_peer_args.implied_port.is_none() {
                                    None
                                } else if announce_peer_args.implied_port.unwrap() {
                                    Some(1)
                                } else {
                                    Some(0)
                                },
                                info_hash: announce_peer_args.info_hash.to_vec(),
                                port: announce_peer_args.port,
                                token: announce_peer_args.token,
                            },
                        }
                    }
                }),

                MessageType::Response(res) => internal::DHTMessageVariant::DHTResponse(match res {
                    ResponseSpecific::FindNodeResponse(find_node_args) => {
                        internal::DHTResponseSpecific::DHTFindNodeResponse {
                            arguments: internal::DHTFindNodeResponseArguments {
                                id: find_node_args.responder_id.to_vec(),
                                nodes: nodes4_to_bytes(&find_node_args.nodes),
                            },
                        }
                    }

                    ResponseSpecific::GetPeersResponse(get_peers_args) => {
                        internal::DHTResponseSpecific::DHTGetPeersResponse {
                            arguments: internal::DHTGetPeersResponseArguments {
                                id: get_peers_args.responder_id.to_vec(),
                                token: get_peers_args.token.clone(),
                                nodes: match &get_peers_args.values {
                                    GetPeersResponseValues::Nodes(nodes) => {
                                        Some(nodes4_to_bytes(&nodes))
                                    }
                                    _ => None,
                                },
                                values: match &get_peers_args.values {
                                    GetPeersResponseValues::Peers(peers) => {
                                        Some(peers_to_bytes(peers))
                                    }
                                    _ => None,
                                },
                            },
                        }
                    }

                    ResponseSpecific::PingResponse(ping_args) => {
                        internal::DHTResponseSpecific::DHTPingResponse {
                            arguments: internal::DHTPingResponseArguments {
                                id: ping_args.responder_id.to_vec(),
                            },
                        }
                    }

                    ResponseSpecific::SampleInfoHashesResponse(sample_info_hashes_args) => {
                        internal::DHTResponseSpecific::DHTSampleInfoHashesResponse {
                            arguments: internal::DHTSampleInfoHashesResponseArguments {
                                id: sample_info_hashes_args.responder_id.to_vec(),
                                interval: std::cmp::min(
                                    MAX_SCRAPE_INTERVAL,
                                    sample_info_hashes_args.interval.as_secs(),
                                ) as i32,
                                num: sample_info_hashes_args.num,
                                nodes: nodes4_to_bytes(&sample_info_hashes_args.nodes),
                                samples: {
                                    let mut a = Vec::with_capacity(
                                        sample_info_hashes_args.samples.len() * ID_SIZE,
                                    );
                                    for info_hash in &sample_info_hashes_args.samples {
                                        a.append(&mut info_hash.to_vec());
                                    }
                                    a
                                },
                            },
                        }
                    }
                }),

                MessageType::Error(err) => {
                    internal::DHTMessageVariant::DHTError(internal::DHTErrorSpecific {
                        error_info: vec![
                            serde_bencode::value::Value::Int(err.code.into()),
                            serde_bencode::value::Value::Bytes(err.description.into()),
                        ],
                    })
                }
            },
        }
    }

    fn from_serde_message(msg: internal::DHTMessage) -> Result<Message, errors::RustyDHTError> {
        Ok(Message {
            transaction_id: msg.transaction_id,
            version: msg.version,
            requester_ip: match msg.ip {
                Some(ip) => Some(bytes_to_sockaddr(ip)?),
                _ => None,
            },
            read_only: match msg.read_only {
                Some(read_only) => Some(read_only >= 1),
                _ => None,
            },

            message_type: match msg.variant {
                internal::DHTMessageVariant::DHTRequest(req_variant) => {
                    MessageType::Request(match req_variant {
                        internal::DHTRequestSpecific::DHTAnnouncePeerRequest { arguments } => {
                            RequestSpecific::AnnouncePeerRequest(AnnouncePeerRequestArguments {
                                requester_id: Id::from_bytes(arguments.id)?,
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
                            })
                        }

                        internal::DHTRequestSpecific::DHTFindNodeRequest { arguments } => {
                            RequestSpecific::FindNodeRequest(FindNodeRequestArguments {
                                requester_id: Id::from_bytes(arguments.id)?,
                                target: Id::from_bytes(&arguments.target)?,
                            })
                        }

                        internal::DHTRequestSpecific::DHTGetPeersRequest { arguments } => {
                            RequestSpecific::GetPeersRequest(GetPeersRequestArguments {
                                requester_id: Id::from_bytes(arguments.id)?,
                                info_hash: Id::from_bytes(&arguments.info_hash)?,
                            })
                        }

                        internal::DHTRequestSpecific::DHTPingRequest { arguments } => {
                            RequestSpecific::PingRequest(PingRequestArguments {
                                requester_id: Id::from_bytes(&arguments.id)?,
                            })
                        }

                        internal::DHTRequestSpecific::DHTSampleInfoHashesRequest { arguments } => {
                            RequestSpecific::SampleInfoHashesRequest(
                                SampleInfoHashesRequestArguments {
                                    requester_id: Id::from_bytes(&arguments.id)?,
                                    target: Id::from_bytes(&arguments.target)?,
                                },
                            )
                        }
                    })
                }

                internal::DHTMessageVariant::DHTResponse(res_variant) => {
                    MessageType::Response(match res_variant {
                        internal::DHTResponseSpecific::DHTFindNodeResponse { arguments } => {
                            ResponseSpecific::FindNodeResponse(FindNodeResponseArguments {
                                responder_id: Id::from_bytes(&arguments.id)?,
                                nodes: bytes_to_nodes4(&arguments.nodes)?,
                            })
                        }

                        internal::DHTResponseSpecific::DHTGetPeersResponse { arguments } => {
                            ResponseSpecific::GetPeersResponse(GetPeersResponseArguments {
                                responder_id: Id::from_bytes(&arguments.id)?,
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
                            })
                        }

                        internal::DHTResponseSpecific::DHTPingResponse { arguments } => {
                            ResponseSpecific::PingResponse(PingResponseArguments {
                                responder_id: Id::from_bytes(&arguments.id)?,
                            })
                        }

                        internal::DHTResponseSpecific::DHTSampleInfoHashesResponse {
                            arguments,
                        } => ResponseSpecific::SampleInfoHashesResponse(
                            SampleInfoHashesResponseArguments {
                                responder_id: Id::from_bytes(&arguments.id)?,
                                interval: Duration::from_secs(arguments.interval as u64),
                                num: arguments.num,
                                nodes: bytes_to_nodes4(&arguments.nodes)?,
                                samples: {
                                    if arguments.samples.len() % ID_SIZE != 0 {
                                        return Err(anyhow!(
                                            "Wrong sample length {} not a multiple of {}",
                                            arguments.samples.len(),
                                            ID_SIZE
                                        )
                                        .into());
                                    }
                                    let num_expected = arguments.samples.len() / ID_SIZE;
                                    let mut to_ret = Vec::with_capacity(num_expected);

                                    for i in 0..num_expected {
                                        let i = i * ID_SIZE;
                                        let id =
                                            Id::from_bytes(&arguments.samples[i..i + ID_SIZE])?;
                                        to_ret.push(id);
                                    }

                                    to_ret
                                },
                            },
                        ),
                    })
                }

                internal::DHTMessageVariant::DHTError(err) => {
                    if err.error_info.len() < 2 {
                        return Err(anyhow!("Error packet should have at least 2 elements").into());
                    }
                    MessageType::Error(ErrorSpecific {
                        code: match err.error_info[0] {
                            serde_bencode::value::Value::Int(code) => match code.try_into() {
                                Ok(code) => code,
                                Err(e) => {
                                    return Err(errors::RustyDHTError::PacketParseError(
                                        anyhow::Error::new(e),
                                    ))
                                }
                            },
                            _ => {
                                return Err(errors::RustyDHTError::PacketParseError(anyhow!(
                                    "Expected error code as first element"
                                )))
                            }
                        },
                        description: match &err.error_info[1] {
                            serde_bencode::value::Value::Bytes(desc) => {
                                match std::str::from_utf8(desc) {
                                    Ok(desc) => desc.to_string(),
                                    Err(e) => {
                                        return Err(errors::RustyDHTError::PacketParseError(
                                            anyhow::Error::new(e),
                                        ))
                                    }
                                }
                            }
                            _ => {
                                return Err(errors::RustyDHTError::PacketParseError(anyhow!(
                                    "Expected description as second element"
                                )))
                            }
                        },
                    })
                }
            },
        })
    }

    pub fn to_bytes(self) -> Result<Vec<u8>, errors::RustyDHTError> {
        self.to_serde_message().to_bytes()
    }

    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Message, errors::RustyDHTError> {
        Message::from_serde_message(internal::DHTMessage::from_bytes(bytes)?)
    }

    pub fn get_author_id(&self) -> Option<Id> {
        let id = match &self.message_type {
            MessageType::Request(request_variant) => match request_variant {
                RequestSpecific::AnnouncePeerRequest(arguments) => arguments.requester_id,
                RequestSpecific::FindNodeRequest(arguments) => arguments.requester_id,
                RequestSpecific::GetPeersRequest(arguments) => arguments.requester_id,
                RequestSpecific::PingRequest(arguments) => arguments.requester_id,
                RequestSpecific::SampleInfoHashesRequest(arguments) => arguments.requester_id,
            },
            MessageType::Response(response_variant) => match response_variant {
                ResponseSpecific::FindNodeResponse(arguments) => arguments.responder_id,
                ResponseSpecific::GetPeersResponse(arguments) => arguments.responder_id,
                ResponseSpecific::PingResponse(arguments) => arguments.responder_id,
                ResponseSpecific::SampleInfoHashesResponse(arguments) => arguments.responder_id,
            },
            MessageType::Error(_) => {
                return None;
            }
        };

        return Some(id);
    }
}

/// Returns true if the response and request types specified match.
/// E.g., PingResponse goes with PingRequest. FindNodeResponse goes with FindNodeRequest.
pub fn response_matches_request(res: &ResponseSpecific, req: &RequestSpecific) -> bool {
    match res {
        ResponseSpecific::PingResponse { .. } => {
            if let RequestSpecific::PingRequest { .. } = req {
                return true;
            }
        }

        ResponseSpecific::FindNodeResponse { .. } => {
            if let RequestSpecific::FindNodeRequest { .. } = req {
                return true;
            }
        }

        ResponseSpecific::GetPeersResponse { .. } => {
            if let RequestSpecific::GetPeersRequest { .. } = req {
                return true;
            }
        }

        _ => {
            warn!(target: "rustydht_lib::response_matches_request",
                "Unimplemented response type {:?}",
                res
            );
        }
    }
    return false;
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

pub fn sockaddr_to_bytes(sockaddr: &SocketAddr) -> Vec<u8> {
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

fn bytes_to_nodes4<T: AsRef<[u8]>>(bytes: T) -> Result<Vec<Node>, errors::RustyDHTError> {
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

fn nodes4_to_bytes(nodes: &Vec<Node>) -> Vec<u8> {
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
    use rand::prelude::*;

    #[test]
    fn test_ping_request() {
        let original_msg = Message {
            transaction_id: vec![0, 1, 2],
            version: None,
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Request(RequestSpecific::PingRequest(
                PingRequestArguments {
                    requester_id: Id::from_hex("f00ff00ff00ff00ff00ff00ff00ff00ff00ff00f").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_ping_response() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![0xde, 0xad]),
            requester_ip: Some("99.100.101.102:1030".parse().unwrap()),
            read_only: None,
            message_type: MessageType::Response(ResponseSpecific::PingResponse(
                PingResponseArguments {
                    responder_id: Id::from_hex("beefbeefbeefbeefbeefbeefbeefbeefbeefbeef").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_get_peers_request() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            version: Some(vec![72, 73]),
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Request(RequestSpecific::GetPeersRequest(
                GetPeersRequestArguments {
                    info_hash: Id::from_hex("deaddeaddeaddeaddeaddeaddeaddeaddeaddead").unwrap(),
                    requester_id: Id::from_hex("beefbeefbeefbeefbeefbeefbeefbeefbeefbeef").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_get_peers_response() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![1]),
            requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
            read_only: None,
            message_type: MessageType::Response(ResponseSpecific::GetPeersResponse(
                GetPeersResponseArguments {
                    responder_id: Id::from_hex("0505050505050505050505050505050505050505").unwrap(),
                    token: vec![99, 100, 101, 102],
                    values: GetPeersResponseValues::Nodes(vec![Node::new(
                        Id::from_hex("0606060606060606060606060606060606060606").unwrap(),
                        "49.50.52.52:5354".parse().unwrap(),
                    )]),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_get_peers_response_peers() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![1]),
            requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
            read_only: None,
            message_type: MessageType::Response(ResponseSpecific::GetPeersResponse(
                GetPeersResponseArguments {
                    responder_id: Id::from_hex("0505050505050505050505050505050505050505").unwrap(),
                    token: vec![99, 100, 101, 102],
                    values: GetPeersResponseValues::Peers(vec!["123.123.123.123:123"
                        .parse()
                        .unwrap()]),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_find_node_request() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![0x62, 0x61, 0x72, 0x66]),
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Request(RequestSpecific::FindNodeRequest(
                FindNodeRequestArguments {
                    target: Id::from_hex("1234123412341234123412341234123412341234").unwrap(),
                    requester_id: Id::from_hex("5678567856785678567856785678567856785678").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_find_node_request_read_only() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![0x62, 0x61, 0x72, 0x66]),
            requester_ip: None,
            read_only: Some(true),
            message_type: MessageType::Request(RequestSpecific::FindNodeRequest(
                FindNodeRequestArguments {
                    target: Id::from_hex("1234123412341234123412341234123412341234").unwrap(),
                    requester_id: Id::from_hex("5678567856785678567856785678567856785678").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_find_node_response() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![1]),
            requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
            read_only: None,
            message_type: MessageType::Response(ResponseSpecific::FindNodeResponse(
                FindNodeResponseArguments {
                    responder_id: Id::from_hex("0505050505050505050505050505050505050505").unwrap(),
                    nodes: vec![Node::new(
                        Id::from_hex("0606060606060606060606060606060606060606").unwrap(),
                        "49.50.52.52:5354".parse().unwrap(),
                    )],
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_announce_peer_request() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![0x62, 0x61, 0x72, 0x66]),
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Request(RequestSpecific::AnnouncePeerRequest(
                AnnouncePeerRequestArguments {
                    requester_id: Id::from_hex("5678567856785678567856785678567856785678").unwrap(),
                    port: 666,
                    implied_port: Some(false),
                    token: vec![42, 42, 42, 42],
                    info_hash: Id::from_hex("9899989998999899989998999899989998999899").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_sample_info_hashes_request() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![0x62, 0x61, 0x72, 0x66]),
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Request(RequestSpecific::SampleInfoHashesRequest(
                SampleInfoHashesRequestArguments {
                    requester_id: Id::from_hex("5678567856785678567856785678567856785678").unwrap(),
                    target: Id::from_hex("3344334433443344334433443344334433443344").unwrap(),
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_sample_info_hashes_response() {
        let original_msg = Message {
            transaction_id: vec![1, 2, 3],
            version: Some(vec![1]),
            requester_ip: Some("50.51.52.53:5455".parse().unwrap()),
            read_only: None,
            message_type: MessageType::Response(ResponseSpecific::SampleInfoHashesResponse(
                SampleInfoHashesResponseArguments {
                    responder_id: Id::from_hex("0505050505050505050505050505050505050505").unwrap(),
                    interval: Duration::from_secs(32),
                    nodes: vec![Node::new(
                        Id::from_hex("0606060606060606060606060606060606060606").unwrap(),
                        "49.50.52.52:5354".parse().unwrap(),
                    )],
                    samples: vec![
                        Id::from_hex("3232323232323232323232323232323232323232").unwrap(),
                        Id::from_hex("3434343434343434343434343434343434343434").unwrap(),
                    ],
                    num: 300,
                },
            )),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_error_response() {
        let original_msg = Message {
            transaction_id: vec![97, 97],
            version: None,
            requester_ip: None,
            read_only: None,
            message_type: MessageType::Error(ErrorSpecific {
                code: 201,
                description: "A Generic Error Occured".to_string(),
            }),
        };

        let serde_msg = original_msg.clone().to_serde_message();
        let bytes = serde_msg.to_bytes().unwrap();
        assert_eq!(
            "d1:eli201e23:A Generic Error Occurede1:t2:aa1:y1:ee",
            String::from_utf8_lossy(&bytes)
        );
        let parsed_serde_msg = internal::DHTMessage::from_bytes(bytes).unwrap();
        let parsed_msg = Message::from_serde_message(parsed_serde_msg).unwrap();
        assert_eq!(parsed_msg, original_msg);
    }

    #[test]
    fn test_response_matches_request_find_node() {
        let res = ResponseSpecific::FindNodeResponse(FindNodeResponseArguments {
            nodes: vec![],
            responder_id: Id::from_random(&mut thread_rng()),
        });
        let req = RequestSpecific::FindNodeRequest(FindNodeRequestArguments {
            requester_id: Id::from_random(&mut thread_rng()),
            target: Id::from_random(&mut thread_rng()),
        });
        assert_eq!(true, response_matches_request(&res, &req));
    }

    #[test]
    fn test_response_matches_request_find_ping() {
        let res = ResponseSpecific::PingResponse(PingResponseArguments {
            responder_id: Id::from_random(&mut thread_rng()),
        });
        let req = RequestSpecific::PingRequest(PingRequestArguments {
            requester_id: Id::from_random(&mut thread_rng()),
        });
        assert_eq!(true, response_matches_request(&res, &req));
    }

    #[test]
    fn test_response_matches_request_find_nonmatching() {
        let res = ResponseSpecific::PingResponse(PingResponseArguments {
            responder_id: Id::from_random(&mut thread_rng()),
        });
        let req = RequestSpecific::FindNodeRequest(FindNodeRequestArguments {
            requester_id: Id::from_random(&mut thread_rng()),
            target: Id::from_random(&mut thread_rng()),
        });
        assert_eq!(false, response_matches_request(&res, &req));
    }
}

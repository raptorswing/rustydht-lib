use serde::{Deserialize, Serialize};

use crate::errors::RustyDHTError;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTMessage {
    #[serde(rename = "t", with = "serde_bytes")]
    pub transaction_id: Vec<u8>,

    #[serde(default)]
    #[serde(rename = "v", with = "serde_bytes")]
    pub version: Option<Vec<u8>>,

    #[serde(flatten)]
    pub variant: DHTMessageVariant,

    #[serde(default)]
    #[serde(with = "serde_bytes")]
    pub ip: Option<Vec<u8>>,
}

impl DHTMessage {
    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<DHTMessage, RustyDHTError> {
        let bytes = bytes.as_ref();
        let obj = serde_bencode::from_bytes(bytes)
            .map_err(|err| RustyDHTError::PacketParseError(err.into()))?;
        Ok(obj)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, RustyDHTError> {
        serde_bencode::to_bytes(self)
            .map_err(|err| RustyDHTError::PacketSerializationError(err.into()))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "y")]
pub enum DHTMessageVariant {
    #[serde(rename = "q")]
    DHTRequest(DHTRequestSpecific),

    #[serde(rename = "r")]
    DHTResponse(DHTResponseSpecific),

    #[serde(rename = "e")]
    DHTError(DHTErrorSpecific),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "q")]
pub enum DHTRequestSpecific {
    #[serde(rename = "ping")]
    DHTPingRequest {
        #[serde(rename = "a")]
        arguments: DHTPingArguments,
    },

    #[serde(rename = "find_node")]
    DHTFindNodeRequest {
        #[serde(rename = "a")]
        arguments: DHTFindNodeArguments,
    },

    #[serde(rename = "get_peers")]
    DHTGetPeersRequest {
        #[serde(rename = "a")]
        arguments: DHTGetPeersArguments,
    },

    #[serde(rename = "announce_peer")]
    DHTAnnouncePeerRequest {
        #[serde(rename = "a")]
        arguments: DHTAnnouncePeerRequestArguments,
    },

    #[serde(rename = "sample_infohashes")]
    DHTSampleInfoHashesRequest {
        #[serde(rename = "a")]
        arguments: DHTSampleInfoHashesRequestArguments,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)] // This means order matters! Order these from most to least detailed
pub enum DHTResponseSpecific {
    DHTGetPeersResponse {
        #[serde(rename = "r")]
        arguments: DHTGetPeersResponseArguments,
    },

    DHTSampleInfoHashesResponse {
        #[serde(rename = "r")]
        arguments: DHTSampleInfoHashesResponseArguments,
    },

    DHTFindNodeResponse {
        #[serde(rename = "r")]
        arguments: DHTFindNodeResponseArguments,
    },

    DHTPingResponse {
        #[serde(rename = "r")]
        arguments: DHTPingResponseArguments,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTErrorSpecific {
    #[serde(rename = "e")]
    error_info: Vec<serde_bencode::value::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTPingArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTFindNodeArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub target: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTGetPeersArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub info_hash: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTSampleInfoHashesRequestArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub target: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTAnnouncePeerRequestArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub info_hash: Vec<u8>,

    pub port: u16,

    #[serde(with = "serde_bytes")]
    pub token: Vec<u8>,

    pub implied_port: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTPingResponseArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTFindNodeResponseArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub nodes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTGetPeersResponseArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub token: Vec<u8>,

    #[serde(with = "serde_bytes")]
    pub nodes: Option<Vec<u8>>,

    pub values: Option<Vec<serde_bytes::ByteBuf>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DHTSampleInfoHashesResponseArguments {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,

    pub interval: i32,

    #[serde(with = "serde_bytes")]
    pub nodes: Vec<u8>,

    pub num: i32,

    #[serde(with = "serde_bytes")]
    pub samples: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    #[test]
    fn test_parse_ping_request() {
        let bytes = hex::decode("64313a6164323a696432303a70f923e90771701587b6d36fbb78b3a8047b092e65313a71343a70696e67313a74323ab51b313a79313a7165").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0xb5, 0x1b],
            ip: None,
            version: None,
            variant: DHTMessageVariant::DHTRequest(DHTRequestSpecific::DHTPingRequest {
                arguments: DHTPingArguments {
                    id: hex::decode("70f923e90771701587b6d36fbb78b3a8047b092e").unwrap(),
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_ping_response() {
        let bytes = hex::decode("64323a6970363a48a8858f2014313a7264323a696432303a70f22fcfdc27e4ff28f0c00a6a3de0d6c4361ccd65313a74323ab51b313a76343a5554b3ba313a79313a7265").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0xb5, 0x1b],
            ip: Some(hex::decode("48a8858f2014").unwrap()),
            version: Some(hex::decode("5554b3ba").unwrap()),
            variant: DHTMessageVariant::DHTResponse(DHTResponseSpecific::DHTPingResponse {
                arguments: DHTPingResponseArguments {
                    id: hex::decode("70f22fcfdc27e4ff28f0c00a6a3de0d6c4361ccd").unwrap(),
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_get_peers_request() {
        let bytes = hex::decode("64313a6164323a696432303a287cbd6f580fcf5920aafe67478a0c882ab2ee8e393a696e666f5f6861736832303a70f986a9fb5a9e8e0bafc62165b717836033210965313a71393a6765745f7065657273313a74343aacc4e52e313a79313a7165").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0xac, 0xc4, 0xe5, 0x2e],
            ip: None,
            version: None,
            variant: DHTMessageVariant::DHTRequest(DHTRequestSpecific::DHTGetPeersRequest {
                arguments: DHTGetPeersArguments {
                    id: hex::decode("287cbd6f580fcf5920aafe67478a0c882ab2ee8e").unwrap(),
                    info_hash: hex::decode("70f986a9fb5a9e8e0bafc62165b7178360332109").unwrap(),
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_get_peers_response() {
        let bytes = hex::decode("64323a6970363a334f9e687d01313a7264323a696432303a70f923e90771701587b6d36fbb78b3a8047b092e353a6e6f6465733230383a70e9325426374aff7f909e8f14faeda58d4f9a68b925f813d37f70eeac0aaef80a095f190dfddd17787eec980182578a6199db0d70ee20a15db5e9c7eee898a14d461fb262e9907c47c213fd1ae970ed932dd5335ab19eaa4f1820814662bb1eea025169e8c1bf6670ed65b14cfcbb854b838465950cc00cd97aeb625d68eb4251f670ec53526e4fe3e0bea78e796ece96b024fd749b05c44818cf0870e0c53d2a3899fd53b2e1ffc8afa95374e0d6cdb0099d9c232770e74ed6ae529049f1f1bbe9ebb3a6db3c870ce152150dd9d8be353a746f6b656e343a76b5550c65313a74323aa0bc313a79313a7265").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec!(0xa0, 0xbc),
            ip: Some(hex::decode("334f9e687d01").unwrap()),
            version: None,
            variant: DHTMessageVariant::DHTResponse(DHTResponseSpecific::DHTGetPeersResponse {
                arguments: DHTGetPeersResponseArguments {
                    id: hex::decode("70f923e90771701587b6d36fbb78b3a8047b092e").unwrap(),
                    nodes: Some(hex::decode("70e9325426374aff7f909e8f14faeda58d4f9a68b925f813d37f70eeac0aaef80a095f190dfddd17787eec980182578a6199db0d70ee20a15db5e9c7eee898a14d461fb262e9907c47c213fd1ae970ed932dd5335ab19eaa4f1820814662bb1eea025169e8c1bf6670ed65b14cfcbb854b838465950cc00cd97aeb625d68eb4251f670ec53526e4fe3e0bea78e796ece96b024fd749b05c44818cf0870e0c53d2a3899fd53b2e1ffc8afa95374e0d6cdb0099d9c232770e74ed6ae529049f1f1bbe9ebb3a6db3c870ce152150dd9d8be").unwrap()),
                    token: hex::decode("76b5550c").unwrap(),
                    values: None,
                }
            })
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_find_node_request() {
        let bytes = hex::decode("64313a6164323a696432303a93b7d318a5034a01231c86f68f385468e67038c6363a74617267657432303a4800711700005e9e00001b190000391d00005a9565313a71393a66696e645f6e6f6465313a74343a41160000313a76343a5554b3ba313a79313a7165").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0x41, 0x16, 0x00, 0x00],
            ip: None,
            version: Some(vec![0x55, 0x54, 0xb3, 0xba]),
            variant: DHTMessageVariant::DHTRequest(DHTRequestSpecific::DHTFindNodeRequest {
                arguments: DHTFindNodeArguments {
                    id: hex::decode("93b7d318a5034a01231c86f68f385468e67038c6").unwrap(),
                    target: hex::decode("4800711700005e9e00001b190000391d00005a95").unwrap(),
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_find_node_response() {
        let bytes = hex::decode("64323a6970363a2969788ee79d313a7264323a696432303a70f923e90771701587b6d36fbb78b3a8047b092e353a6e6f6465733230383a4bcf558e1e7f7543f63f6cd57ca9c50d446e898d5f5499481ae14bf18f4fa61e6fc2a944b024511bfc0f67997eff8e86c3565f854f65edbc03f23184dedf8d8619b0916bbcaca617d5855fc71ae14fb2b2feff1694d9a3ccbda1c68dbe4efb62f08f5f58f6cb1ae142024361c0ecf637b4125af472a8214fe49c9603a93f15571ae142db840f263413b8a8603e1e87fea7cc0bf1bd0c3216850a1ae15b506431ba2475bbe30a243217c091adafe45f563b157ed71f525bc83812fadb89e6d47df829a327b951c9b726a9a93dda2d1ae165313a74343a41160000313a79313a7265").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
                transaction_id: vec!(0x41, 0x16, 0x00, 0x00),
                ip: Some(hex::decode("2969788ee79d").unwrap()),
                version: None,
                variant: DHTMessageVariant::DHTResponse(DHTResponseSpecific::DHTFindNodeResponse {
                    arguments: DHTFindNodeResponseArguments {
                        id: hex::decode("70f923e90771701587b6d36fbb78b3a8047b092e").unwrap(),
                        nodes: hex::decode("4bcf558e1e7f7543f63f6cd57ca9c50d446e898d5f5499481ae14bf18f4fa61e6fc2a944b024511bfc0f67997eff8e86c3565f854f65edbc03f23184dedf8d8619b0916bbcaca617d5855fc71ae14fb2b2feff1694d9a3ccbda1c68dbe4efb62f08f5f58f6cb1ae142024361c0ecf637b4125af472a8214fe49c9603a93f15571ae142db840f263413b8a8603e1e87fea7cc0bf1bd0c3216850a1ae15b506431ba2475bbe30a243217c091adafe45f563b157ed71f525bc83812fadb89e6d47df829a327b951c9b726a9a93dda2d1ae1").unwrap(),
                    }
                })
            };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_announce_peer_request() {
        let bytes = hex::decode("64313a6164323a696432303aa4df50d016f245e487bd6c2ce324922b5e2a7e56393a696e666f5f6861736832303a70f936a4d868a160b3d28e258a949da5269f95dd343a706f727469313139313165353a746f6b656e343ad0ed2ab965313a7131333a616e6e6f756e63655f70656572313a74343a1d4c0100313a79313a7165").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: hex::decode("1d4c0100").unwrap(),
            ip: None,
            version: None,
            variant: DHTMessageVariant::DHTRequest(DHTRequestSpecific::DHTAnnouncePeerRequest {
                arguments: DHTAnnouncePeerRequestArguments {
                    id: hex::decode("a4df50d016f245e487bd6c2ce324922b5e2a7e56").unwrap(),
                    info_hash: hex::decode("70f936a4d868a160b3d28e258a949da5269f95dd").unwrap(),
                    token: hex::decode("d0ed2ab9").unwrap(),
                    implied_port: None,
                    port: 11911,
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_announce_peer_response() {
        let bytes = hex::decode("64323a6970363a725c2f242e87313a7264323a696432303a70f923e90771701587b6d36fbb78b3a8047b092e65313a74343a1d4c0100313a79313a7265").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        // Announce peer response is the same as ping response
        let legit = DHTMessage {
            transaction_id: hex::decode("1d4c0100").unwrap(),
            ip: Some(hex::decode("725c2f242e87").unwrap()),
            version: None,
            variant: DHTMessageVariant::DHTResponse(DHTResponseSpecific::DHTPingResponse {
                arguments: DHTPingResponseArguments {
                    id: hex::decode("70f923e90771701587b6d36fbb78b3a8047b092e").unwrap(),
                },
            }),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_sample_info_hashes_request() {
        let bytes = hex::decode("64313a6164323a696432303a0000000000000000000000000000000000000000363a74617267657432303abad6487806506bd534e78a8e375e3f59f7bb5ee065313a7131373a73616d706c655f696e666f686173686573313a74323a6161313a79313a7165").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0x61, 0x61],
            ip: None,
            version: None,
            variant: DHTMessageVariant::DHTRequest(
                DHTRequestSpecific::DHTSampleInfoHashesRequest {
                    arguments: DHTSampleInfoHashesRequestArguments {
                        id: hex::decode("0000000000000000000000000000000000000000").unwrap(),
                        target: hex::decode("bad6487806506bd534e78a8e375e3f59f7bb5ee0").unwrap(),
                    },
                },
            ),
        };

        assert_eq!(legit, message);
    }

    #[test]
    fn test_parse_sample_info_hashes_response() {
        let bytes = hex::decode("64323a6970363aae886914199c313a7264323a696432303a70f923e90771701587b6d36fbb78b3a8047b092e383a696e74657276616c69313065353a6e6f6465733230383aba8eaeb7d0887e2188829a3b301cefea04ac42bd77f68a2f219eafa5bc6bbde9b4be57bb55f89ed3d8a040c773864c4424a316d9af02e5b45e830a87fc5850c4a9f2897930b0e5ee7b0174141ae9adadc1d6ae529049f1f1bbe9ebb3a6db3c870ce19a05e5b689e59c740634b8349528bdbe6b873bb286953695a42cd3036e57d837d17ced58f32c648428f5fabddcce08322e734d0dcbf3065e1f56c3ea2fb8518d118a82e98efe8fb449f6272075d9d95b1efa5655c40e862e5a67a1ee07cc392391f1150fd98c5e0356860efb1ae1333a6e756d69353065373a73616d706c6573313030303a70f9016c33064c1588b7fc72ba51d16b7a808c5170fdb5e6b8330a133375825453f43325dc1179e570f9a5683c028a3285edb48e0054ab0297171a7370f923768792cdda855d52d8f94674a4b085471d70f94e390702f1752df4d22b56b0beb6201b1da370fbfd6ee5f04066826bb483934296fd93e965f670fa23ab785c8958e95e97055a6443af8b27395e70fd94fd6e8833fdc708f54c913384c255fdcc8f70f870271cbcbae8e168e718de24518509bdd4ca70f2fba18423616ec25af3f20b1e331a050ad0da70f95bd4331fa413d7b8c5105a5b40e2dec6b2117013b667f0d899711de54b18b6aac4eeb2a20bca70b3119918544af1550f541a273cc5066fa3749670deaece52a2e7cbd720d86738b1d9940d41a41470ea875f2440ca33c972beca8796036a07b203fc71e3097589ddc215091f24f46d0e921796faf89d70edfb3f69a41c9ecfa01a885f4071ebbca1301570c28d739c5bca06ab10c69122fa6eb7efbf3aa072a5bcd8cc6b6ef3bd7fbcd1aa6ee5f8b93d605870f116deb1571be860e93968b505e88fb823d835706e206f7e7ee0427ec411923b5f9c4f2f6206f670fdb9eb81aea469579f6507cf6d77c919f5063c754361e374a06a314bcbe035105ddaa683948b1570e0f10f444494739d93f0a99921ae4f65746f0a70f4b7663fc542817d374e6aab9c6c672d6caeb27022f4947d059fb44d5e77847593148675bb461c70f828a524473b9fc47dc1f6bd03c531e29b02a0579692958a7ef01ec5fd2fec19d6e6a5d70f73cd69c63da6c452d66a1f07764e560cdf481c03926e70f9ab992cfcaf44f019a4e7e3a1fa156310953970fca0330dc5a8be0b049c355d757c018e84a69d70f936a4d868a160b3d28e258a949da5269f95dd70ea1254e774f4de28f1958130ae29e1b0ae676705239c876fc0625b0747c15bafa9c74c54b1c59f70f9700293c36bd5cd577809ce1bbfd095f68c3470fb25dca1bb972adf030198a86df35d4ed5895470f92e4cb578dbbbb597d5ce817800cc0b16ffb670fbc1fce7ebe963801a0ae3cbe458d4d0a7610d7078d197e43ec4241fb64113e40de762caa4ec3770f922d83be65f62b529af80863ce95633dea31670ae40c35ade9e78a221ea03374166713f0e978870fb102aecf0d60667da804c65133daf6ee6535170f92673005393101e1eb72a40867ee9ab593a6a7093103de4f85fa0b129ce581a9ec8d1bceacd8170e9e99dddae872c6db7b8f59a5f2e75042bee1070c38852f2a1a4daf2cf26bdf35f50726784c36e70f9f09736a704b5ba429f2fcbc9b3bcef01d9db70fa495a44064f605aae7a0426f1bc79c973589870fa4174330e8af3de839f67b2648c39c2ee80c370b44351557950999a0512211ae4d5b8d34d7e1365313a74323a6161313a79313a7265").unwrap();
        let message = DHTMessage::from_bytes(bytes).expect("Failed to parse");

        let legit = DHTMessage {
            transaction_id: vec![0x61, 0x61],
            ip: Some(hex::decode("ae886914199c").unwrap()),
            version: None,
            variant: DHTMessageVariant::DHTResponse(DHTResponseSpecific::DHTSampleInfoHashesResponse {
                arguments: DHTSampleInfoHashesResponseArguments {
                    id: hex::decode("70f923e90771701587b6d36fbb78b3a8047b092e").unwrap(),
                    interval: 10,
                    nodes: hex::decode("ba8eaeb7d0887e2188829a3b301cefea04ac42bd77f68a2f219eafa5bc6bbde9b4be57bb55f89ed3d8a040c773864c4424a316d9af02e5b45e830a87fc5850c4a9f2897930b0e5ee7b0174141ae9adadc1d6ae529049f1f1bbe9ebb3a6db3c870ce19a05e5b689e59c740634b8349528bdbe6b873bb286953695a42cd3036e57d837d17ced58f32c648428f5fabddcce08322e734d0dcbf3065e1f56c3ea2fb8518d118a82e98efe8fb449f6272075d9d95b1efa5655c40e862e5a67a1ee07cc392391f1150fd98c5e0356860efb1ae1").unwrap(),
                    num: 50,
                    samples: hex::decode("70f9016c33064c1588b7fc72ba51d16b7a808c5170fdb5e6b8330a133375825453f43325dc1179e570f9a5683c028a3285edb48e0054ab0297171a7370f923768792cdda855d52d8f94674a4b085471d70f94e390702f1752df4d22b56b0beb6201b1da370fbfd6ee5f04066826bb483934296fd93e965f670fa23ab785c8958e95e97055a6443af8b27395e70fd94fd6e8833fdc708f54c913384c255fdcc8f70f870271cbcbae8e168e718de24518509bdd4ca70f2fba18423616ec25af3f20b1e331a050ad0da70f95bd4331fa413d7b8c5105a5b40e2dec6b2117013b667f0d899711de54b18b6aac4eeb2a20bca70b3119918544af1550f541a273cc5066fa3749670deaece52a2e7cbd720d86738b1d9940d41a41470ea875f2440ca33c972beca8796036a07b203fc71e3097589ddc215091f24f46d0e921796faf89d70edfb3f69a41c9ecfa01a885f4071ebbca1301570c28d739c5bca06ab10c69122fa6eb7efbf3aa072a5bcd8cc6b6ef3bd7fbcd1aa6ee5f8b93d605870f116deb1571be860e93968b505e88fb823d835706e206f7e7ee0427ec411923b5f9c4f2f6206f670fdb9eb81aea469579f6507cf6d77c919f5063c754361e374a06a314bcbe035105ddaa683948b1570e0f10f444494739d93f0a99921ae4f65746f0a70f4b7663fc542817d374e6aab9c6c672d6caeb27022f4947d059fb44d5e77847593148675bb461c70f828a524473b9fc47dc1f6bd03c531e29b02a0579692958a7ef01ec5fd2fec19d6e6a5d70f73cd69c63da6c452d66a1f07764e560cdf481c03926e70f9ab992cfcaf44f019a4e7e3a1fa156310953970fca0330dc5a8be0b049c355d757c018e84a69d70f936a4d868a160b3d28e258a949da5269f95dd70ea1254e774f4de28f1958130ae29e1b0ae676705239c876fc0625b0747c15bafa9c74c54b1c59f70f9700293c36bd5cd577809ce1bbfd095f68c3470fb25dca1bb972adf030198a86df35d4ed5895470f92e4cb578dbbbb597d5ce817800cc0b16ffb670fbc1fce7ebe963801a0ae3cbe458d4d0a7610d7078d197e43ec4241fb64113e40de762caa4ec3770f922d83be65f62b529af80863ce95633dea31670ae40c35ade9e78a221ea03374166713f0e978870fb102aecf0d60667da804c65133daf6ee6535170f92673005393101e1eb72a40867ee9ab593a6a7093103de4f85fa0b129ce581a9ec8d1bceacd8170e9e99dddae872c6db7b8f59a5f2e75042bee1070c38852f2a1a4daf2cf26bdf35f50726784c36e70f9f09736a704b5ba429f2fcbc9b3bcef01d9db70fa495a44064f605aae7a0426f1bc79c973589870fa4174330e8af3de839f67b2648c39c2ee80c370b44351557950999a0512211ae4d5b8d34d7e13").unwrap(),
                }
            })
        };

        assert_eq!(legit, message);
    }
}

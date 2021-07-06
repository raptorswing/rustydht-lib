use thiserror::Error;

#[derive(Error, Debug)]
pub enum RustyDHTError {
    // Failure to parse bytes of a packet
    #[error("Failed to parse packet bytes: {0}")]
    PacketParseError(#[from] anyhow::Error),

    #[error("Failed to serialize msg: {0}")]
    PacketSerializationError(#[from] serde_bencode::Error),
}

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RustyDHTError {
    // Failure to parse bytes of a packet
    #[error("Failed to parse packet bytes: {0}")]
    PacketParseError(#[from] anyhow::Error),

    #[error("Failed to serialize msg: {0}")]
    PacketSerializationError(#[from] serde_bencode::Error),

    #[error("General error: {0}")]
    GeneralError(#[source] anyhow::Error),

    #[error("Connection tracking error: {0}")]
    ConntrackError(#[source] anyhow::Error),

    #[error("Socket send error: {0}")]
    SocketSendError(#[source] std::io::Error),

    #[error("Socket recv error: {0}")]
    SocketRecvError(#[source] std::io::Error),

    #[error("Operation timed out: {0}")]
    TimeoutError(#[source] anyhow::Error),

    /// This error is a hack for signaling shutdown.
    /// Don't use unless you're sure you know what you're doing.
    #[error("It's time to shutdown tasks: {0}")]
    ShutdownError(#[source] anyhow::Error),

    /// Indicates that the Message type you're trying to build requires more information.
    #[error("{0} is required")]
    BuilderMissingFieldError(&'static str),

    /// Indicates that the builder is in an invalid/ambiguous state to build the desired
    /// Message type.
    #[error("Builder state invalid: {0}")]
    BuilderInvalidComboError(&'static str),
}

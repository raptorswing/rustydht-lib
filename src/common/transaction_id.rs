#[derive(PartialEq, Eq, Hash)]
/// Represents a DHT transaction id, which are basically just small byte strings.
/// This type is not yet used widely across this codebase.
pub struct TransactionId {
    pub bytes: Vec<u8>,
}

impl From<Vec<u8>> for TransactionId {
    fn from(tid: Vec<u8>) -> Self {
        TransactionId { bytes: tid }
    }
}

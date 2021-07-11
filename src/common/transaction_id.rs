#[derive(PartialEq, Eq, Hash)]
pub struct TransactionId {
    pub bytes: Vec<u8>,
}

impl From<Vec<u8>> for TransactionId {
    fn from(tid: Vec<u8>) -> Self {
        return TransactionId { bytes: tid };
    }
}

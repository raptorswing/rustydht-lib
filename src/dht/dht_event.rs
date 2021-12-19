use crate::packets::Message;

#[derive(Debug, PartialEq, Clone)]
pub struct DHTEvent {
    pub event_type: DHTEventType,
}

#[derive(Debug, PartialEq, Clone)]
pub enum DHTEventType {
    MessageReceived(MessageReceivedEvent),
}

#[derive(Debug, PartialEq, Clone)]
pub struct MessageReceivedEvent {
    pub message: Message,
}

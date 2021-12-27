use crate::packets::Message;

/// Top-level message that [DHT](crate::dht::DHT) will send to callers that
/// [subscribe](crate::dht::DHT::subscribe) to events.
#[derive(Debug, PartialEq, Clone)]
pub struct DHTEvent {
    pub event_type: DHTEventType,
}

/// Enum that represents the different types of events that can be sent from the DHT.
#[derive(Debug, PartialEq, Clone)]
pub enum DHTEventType {
    MessageReceived(MessageReceivedEvent),
}

/// This struct is used when [DHT](crate::dht::DHT) receives a message from another
/// node on the DHT.
///
/// The event is sent after the DHT has finished all other processing for the message.
#[derive(Debug, PartialEq, Clone)]
pub struct MessageReceivedEvent {
    pub message: Message,
}

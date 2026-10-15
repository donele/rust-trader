use core::Message;

/// A source of messages that is already ordered within its own channel.
///
/// Concrete sources provide messages; the sequencer owns their runtime state
/// and merges their streams using [`MessageSource`] ordering metadata.
pub trait MessageSource {
    fn next_message(&mut self) -> Option<Message>;

    fn source_name(&self) -> &str {
        "SOURCE"
    }
}

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::{MessageSource, VecMessageSource};
use core::{Message, TimeNanos};

/// Runtime state associated with one message source in the sequencer.
///
/// The channel owns the source and buffers its next message while that message
/// waits in the global schedule.
struct Channel {
    name: String,
    priority: i64,
    source: Box<dyn MessageSource>,
    next_message: Option<Message>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Lightweight ordering metadata for one buffered channel message.
///
/// The heap stores these entries instead of the full source state. The channel
/// index points back to the corresponding [`Channel`] in `SimSequencer`.
struct HeapEntry {
    timestamp: TimeNanos,
    priority: i64,
    channel_index: usize,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .timestamp
            .cmp(&self.timestamp)
            .then_with(|| self.priority.cmp(&other.priority))
            .then_with(|| other.channel_index.cmp(&self.channel_index))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
pub struct SimSequencer {
    channels: Vec<Channel>,
    schedule: BinaryHeap<HeapEntry>,
    initialized: bool,
    start_time: Option<TimeNanos>,
    end_time: Option<TimeNanos>,
}

impl std::fmt::Debug for SimSequencer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SimSequencer")
            .field("channels", &self.channels.len())
            .field("scheduled", &self.schedule.len())
            .field("initialized", &self.initialized)
            .field("start_time", &self.start_time)
            .field("end_time", &self.end_time)
            .finish()
    }
}

impl SimSequencer {
    pub fn new(events: Vec<Message>) -> Self {
        let mut sequencer = Self::default();
        sequencer.add_source("UNNAMED", 0, Box::new(VecMessageSource::new(events)));
        sequencer
    }

    pub fn add_source(
        &mut self,
        name: impl Into<String>,
        priority: i64,
        source: Box<dyn MessageSource>,
    ) {
        assert!(
            !self.initialized,
            "sources must be added before sequencing starts"
        );
        self.channels.push(Channel {
            name: name.into(),
            priority,
            source,
            next_message: None,
        });
    }

    pub fn set_time_window(&mut self, start_time: Option<TimeNanos>, end_time: Option<TimeNanos>) {
        assert!(
            !self.initialized,
            "time window must be set before sequencing starts"
        );
        self.start_time = start_time;
        self.end_time = end_time;
    }

    fn initialize(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        for index in 0..self.channels.len() {
            let channel = &mut self.channels[index];
            channel.next_message = channel.source.next_message();
            if let Some(message) = channel.next_message {
                self.schedule.push(HeapEntry {
                    timestamp: message.ts_nanos(),
                    priority: channel.priority,
                    channel_index: index,
                });
            }
        }
    }

    pub fn next_message(&mut self) -> Option<Message> {
        self.initialize();
        loop {
            let entry = self.schedule.pop()?;
            let channel = &mut self.channels[entry.channel_index];
            let message = channel
                .next_message
                .take()
                .expect("scheduled channel has a message");
            channel.next_message = channel.source.next_message();
            if let Some(next) = channel.next_message {
                self.schedule.push(HeapEntry {
                    timestamp: next.ts_nanos(),
                    priority: channel.priority,
                    channel_index: entry.channel_index,
                });
            }
            if self.end_time.is_some_and(|end| message.ts_nanos() >= end) {
                return None;
            }
            if self
                .start_time
                .is_some_and(|start| message.ts_nanos() < start)
            {
                continue;
            }
            return Some(message);
        }
    }

    pub fn peek_next_message(&mut self) -> Option<Message> {
        self.initialize();
        let entry = self.schedule.peek()?;
        self.channels[entry.channel_index].next_message
    }

    /// Drives the ordered event stream through a runtime handler.
    pub fn run<E, F>(&mut self, mut handler: F) -> Result<(), E>
    where
        F: FnMut(Message) -> Result<(), E>,
    {
        while let Some(message) = self.next_message() {
            handler(message)?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.schedule.len()
    }
    pub fn is_empty(&self) -> bool {
        self.schedule.is_empty() && self.initialized
    }
    pub fn drain(mut self) -> Vec<Message> {
        std::iter::from_fn(|| self.next_message()).collect()
    }
    pub fn channel_names(&self) -> impl Iterator<Item = &str> {
        self.channels.iter().map(|channel| channel.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{Side, SymbolId};

    fn trade(timestamp: TimeNanos) -> Message {
        Message::Trade(core::Trade {
            ts_nanos: timestamp,
            symbol: SymbolId(1),
            price: 100.0,
            qty: 1.0,
            trade_id: timestamp,
            side: Side::Buy,
            is_market_maker: false,
        })
    }

    #[test]
    fn sequencer_merges_by_time_then_priority_then_channel_index() {
        let mut sequencer = SimSequencer::default();
        sequencer.add_source(
            "LOW",
            1,
            Box::new(VecMessageSource::new(vec![trade(10), trade(20)])),
        );
        sequencer.add_source(
            "HIGH",
            2,
            Box::new(VecMessageSource::new(vec![trade(10), trade(15)])),
        );
        let messages = sequencer.drain();
        let timestamps: Vec<_> = messages.iter().map(Message::ts_nanos).collect();
        assert_eq!(timestamps, vec![10, 10, 15, 20]);
        assert_eq!(messages[0], trade(10));
        assert_eq!(messages[1], trade(10));
    }

    #[test]
    fn sequencer_applies_half_open_time_window() {
        let mut sequencer = SimSequencer::new(vec![trade(9), trade(10), trade(19), trade(20)]);
        sequencer.set_time_window(Some(10), Some(20));
        let messages = sequencer.drain();
        let timestamps: Vec<_> = messages.iter().map(Message::ts_nanos).collect();
        assert_eq!(timestamps, vec![10, 19]);
    }

    #[test]
    fn sequencer_stops_at_end_time() {
        let mut sequencer = SimSequencer::new(vec![trade(10), trade(20), trade(30)]);
        sequencer.set_time_window(None, Some(20));

        assert_eq!(sequencer.next_message().unwrap().ts_nanos(), 10);
        assert!(sequencer.next_message().is_none());
        assert_eq!(sequencer.len(), 1);
    }
}

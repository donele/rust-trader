use std::cmp::Ordering;
use std::collections::BinaryHeap;

use core::{Message, StrategyId, SymbolId, TimeNanos};
use feed::{MessageSource, SourceSpec};
use sim_exchange::{ExchangeMessageSink, ExchangeSimulator};
use strategy::Dispatcher;

/// Runtime state for one source. Its next message is buffered in the global
/// simulation queue while the source itself remains owned by the sequencer.
struct Channel {
    priority: i64,
    source: Box<dyn MessageSource>,
    next_message: Option<Message>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PendingMessage {
    Source { channel_index: usize },
    Generated { message: Message },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct QueueEntry {
    strategy_time: TimeNanos,
    priority: i64,
    sequence: u64,
    pending: PendingMessage,
}

impl Eq for QueueEntry {}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .strategy_time
            .cmp(&self.strategy_time)
            .then_with(|| self.priority.cmp(&other.priority))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default)]
struct EventQueue {
    entries: BinaryHeap<QueueEntry>,
    next_sequence: u64,
}

impl EventQueue {
    fn push(&mut self, strategy_time: TimeNanos, priority: i64, pending: PendingMessage) {
        self.entries.push(QueueEntry {
            strategy_time,
            priority,
            sequence: self.next_sequence,
            pending,
        });
        self.next_sequence += 1;
    }

    fn pop(&mut self) -> Option<QueueEntry> {
        self.entries.pop()
    }
}

impl ExchangeMessageSink for EventQueue {
    fn enqueue(&mut self, message: Message, strategy_time: TimeNanos, priority: i64) {
        self.push(
            strategy_time,
            priority,
            PendingMessage::Generated { message },
        );
    }
}

/// The simulation-only coordinator and owner of the simulation event queue.
///
/// It owns both source channels and exchange-generated messages, so all events
/// share one ordering mechanism before entering the exchange and dispatcher.
pub struct SimulationSequencer {
    channels: Vec<Channel>,
    queue: EventQueue,
    exchange: ExchangeSimulator,
    dispatcher: Dispatcher,
    initialized: bool,
    start_time: Option<TimeNanos>,
    end_time: Option<TimeNanos>,
    events_processed: usize,
    source_messages: usize,
}

impl std::fmt::Debug for SimulationSequencer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SimulationSequencer")
            .field("channels", &self.channels.len())
            .field("queued_events", &self.queue.entries.len())
            .field("initialized", &self.initialized)
            .field("start_time", &self.start_time)
            .field("end_time", &self.end_time)
            .finish()
    }
}

impl SimulationSequencer {
    pub fn new(registry: &core::symbols::SymbolRegistry, strategy_id: StrategyId) -> Self {
        Self {
            channels: Vec::new(),
            queue: EventQueue::default(),
            exchange: ExchangeSimulator::new(),
            dispatcher: Dispatcher::new(registry, strategy_id),
            initialized: false,
            start_time: None,
            end_time: None,
            events_processed: 0,
            source_messages: 0,
        }
    }

    pub fn add_source(
        &mut self,
        _name: impl Into<String>,
        priority: i64,
        source: Box<dyn MessageSource>,
    ) {
        assert!(
            !self.initialized,
            "sources must be added before sequencing starts"
        );
        self.channels.push(Channel {
            priority,
            source,
            next_message: None,
        });
    }

    pub fn add_sources(&mut self, sources: impl IntoIterator<Item = SourceSpec>) {
        for source in sources {
            self.add_source(source.name, source.priority, source.source);
        }
    }

    #[allow(dead_code)]
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
            self.channels[index].next_message = self.channels[index].source.next_message();
            self.enqueue_next_source_message(index);
        }
    }

    fn enqueue_next_source_message(&mut self, channel_index: usize) {
        let channel = &self.channels[channel_index];
        if let Some(message) = channel.next_message {
            self.queue.push(
                message.ts_nanos(),
                channel.priority,
                PendingMessage::Source { channel_index },
            );
        }
    }

    fn next_message(&mut self) -> Option<Message> {
        self.initialize();
        loop {
            let entry = self.queue.pop()?;
            let message = match entry.pending {
                PendingMessage::Generated { message } => message,
                PendingMessage::Source { channel_index } => {
                    let channel = &mut self.channels[channel_index];
                    let message = channel
                        .next_message
                        .take()
                        .expect("scheduled channel has a message");
                    channel.next_message = channel.source.next_message();
                    self.enqueue_next_source_message(channel_index);
                    self.source_messages += 1;
                    message
                }
            };

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

    pub fn run(&mut self) -> Result<(), String> {
        while let Some(message) = self.next_message() {
            self.events_processed += 1;
            self.exchange.process_message(&message);
            self.dispatcher
                .process(&message)
                .map_err(|error| format!("dispatch message: {error:?}"))?;
        }
        Ok(())
    }

    pub fn book(&self, symbol_id: SymbolId) -> Option<&core::book::OrderBook> {
        self.dispatcher.book(symbol_id)
    }

    pub fn events_processed(&self) -> usize {
        self.events_processed
    }

    pub fn source_messages(&self) -> usize {
        self.source_messages
    }

    pub fn order_updates_seen(&self) -> usize {
        self.dispatcher.order_updates_seen()
    }
}

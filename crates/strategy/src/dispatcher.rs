use core::book::{BookError, BookMap, OrderBook};
use core::symbols::SymbolRegistry;
use core::{Message, StrategyId, SymbolId};

use crate::StrategyHost;

/// Routes a shared message to the matching typed strategy-host handler.
#[derive(Debug)]
pub struct Dispatcher {
    books: BookMap,
    strategy_host: StrategyHost,
}

#[derive(Debug)]
pub enum DispatchError {
    Book(BookError),
}

impl From<BookError> for DispatchError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl Dispatcher {
    pub fn new(registry: &SymbolRegistry, strategy_id: StrategyId) -> Self {
        Self::with_strategy(registry, StrategyHost::new(strategy_id))
    }

    pub fn with_strategy(registry: &SymbolRegistry, strategy_host: StrategyHost) -> Self {
        Self {
            books: BookMap::from_registry(registry),
            strategy_host,
        }
    }

    /// Applies a message to the client-visible book and then routes it to the host.
    pub fn process(&mut self, message: &Message) -> Result<(), DispatchError> {
        self.books.process(message)?;
        Self::route(&mut self.strategy_host, message);
        Ok(())
    }

    pub fn route(host: &mut StrategyHost, message: &Message) {
        match message {
            Message::Incremental(value) => host.on_incremental(value),
            Message::BookTicker(value) => host.on_book_ticker(value),
            Message::Trade(value) => host.on_trade(value),
            Message::FundingRate(value) => host.on_funding_rate(value),
            Message::Liquidation(value) => host.on_liquidation(value),
            Message::Clock(value) => host.on_clock(value),
            Message::EndTime(value) => host.on_end_time(value),
            Message::PacketBegin(value) => host.on_packet_begin(value),
            Message::PacketEnd(value) => host.on_packet_end(value),
            Message::Reset(value) => host.on_reset(value),
            Message::AlgoOrderRequest(value) => host.on_algo_order_request(value),
            Message::OrderUpdate(value) => host.on_order_update(value),
        }
    }

    pub fn book(&self, symbol_id: SymbolId) -> Option<&OrderBook> {
        self.books.get(symbol_id)
    }

    pub fn order_updates_seen(&self) -> usize {
        self.strategy_host.order_updates_seen()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use core::{BookTicker, Message, StrategyId, SymbolId};

    use super::*;
    use crate::Strategy;

    #[derive(Debug, Clone)]
    struct RecordingStrategy {
        ticker_count: Arc<AtomicUsize>,
    }

    impl Strategy for RecordingStrategy {
        fn on_book_ticker(&mut self, _: &BookTicker) {
            self.ticker_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn dispatch_routes_book_ticker_to_strategy() {
        let ticker_count = Arc::new(AtomicUsize::new(0));
        let strategy = RecordingStrategy {
            ticker_count: Arc::clone(&ticker_count),
        };
        let mut registry = SymbolRegistry::default();
        registry
            .register(core::symbols::Symbol::new(
                SymbolId(1),
                "ABC",
                "TEST",
                0.01,
                0.01,
            ))
            .unwrap();
        let mut dispatcher = Dispatcher::with_strategy(
            &registry,
            StrategyHost::with_strategy(StrategyId(1), Box::new(strategy)),
        );
        let message = Message::BookTicker(BookTicker {
            ts_nanos: 1,
            symbol: SymbolId(1),
            best_bid_price: 100.0,
            best_bid_qty: 1.0,
            best_ask_price: 101.0,
            best_ask_qty: 1.0,
            update_id: 1,
        });

        dispatcher.process(&message).unwrap();
        assert_eq!(ticker_count.load(Ordering::Relaxed), 1);
    }
}

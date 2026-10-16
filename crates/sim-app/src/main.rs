use std::path::{Path, PathBuf};

use core::symbols::{Symbol, SymbolRegistry};
use core::{Message, StrategyId, SymbolId};
use feed::{
    EventPayload, EventTarget, MarketDataCsvKind, MarketDataCsvSource, SimSequencer,
    SimulationEventQueue,
};
use sim_exchange::ExchangeSimulator;
use strategy::Dispatcher;

const DEFAULT_DATA_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/Ferris_csv/XYZX/2026/1011"
);

fn main() {
    if let Err(error) = run_ferris_data(Path::new(DEFAULT_DATA_ROOT)) {
        eprintln!("trade_sim failed: {error}");
        std::process::exit(1);
    }
}

fn run_ferris_data(root: &Path) -> Result<(), String> {
    let registry = symbol_registry()?;
    let mut sequencer = SimSequencer::default();
    add_ferris_sources(&mut sequencer, root, "ABC__XYZX", SymbolId(1), 0.01, 0.01)?;
    add_ferris_sources(&mut sequencer, root, "DEF__XYZX", SymbolId(2), 0.0001, 0.1)?;

    let mut queue = SimulationEventQueue::new(sequencer);
    let mut simulator = ExchangeSimulator::new();
    let mut dispatcher = Dispatcher::new(&registry, StrategyId(2));
    let mut events_processed = 0usize;
    let mut source_messages = 0usize;

    while let Some(event) = queue.next_event() {
        events_processed += 1;
        match (event.target, event.payload) {
            (EventTarget::Exchange, EventPayload::Message(message)) => {
                source_messages += 1;
                let client_message = simulator.on_market_data(message);
                queue.push_client_message(client_message.ts_nanos(), 0, client_message);
            }
            (EventTarget::Exchange, EventPayload::OrderRequest(request)) => {
                let update = simulator.submit(request);
                queue.push_client_message(update.ts_nanos, 0, Message::OrderUpdate(update));
            }
            (EventTarget::Client, EventPayload::Message(message)) => {
                dispatcher
                    .process(&message)
                    .map_err(|error| format!("dispatch message: {error:?}"))?
            }
            (EventTarget::Client, EventPayload::OrderRequest(_)) => {
                unreachable!("OrderRequest is internal to the exchange")
            }
        }
    }

    println!("trade_sim complete");
    println!("data_root={}", root.display());
    println!("source_messages={source_messages}");
    println!("events_processed={events_processed}");
    println!("order_updates_seen={}", dispatcher.order_updates_seen());
    for symbol_id in [SymbolId(1), SymbolId(2)] {
        let book = dispatcher
            .book(symbol_id)
            .ok_or_else(|| format!("missing book for {symbol_id:?}"))?;
        let bid = book.best_bid().map(|level| level.price.to_value());
        let ask = book.best_ask().map(|level| level.price.to_value());
        println!("{symbol_id:?}: best_bid={bid:?} best_ask={ask:?}");
    }
    Ok(())
}

fn symbol_registry() -> Result<SymbolRegistry, String> {
    let mut registry = SymbolRegistry::default();
    registry
        .register(Symbol::new(SymbolId(1), "ABC__XYZX", "XYZX", 0.01, 0.01))
        .map_err(|error| format!("register ABC: {error:?}"))?;
    registry
        .register(Symbol::new(SymbolId(2), "DEF__XYZX", "XYZX", 0.0001, 0.1))
        .map_err(|error| format!("register DEF: {error:?}"))?;
    Ok(registry)
}

fn add_ferris_sources(
    sequencer: &mut SimSequencer,
    root: &Path,
    flat_id: &str,
    symbol: SymbolId,
    price_increment: f64,
    qty_increment: f64,
) -> Result<(), String> {
    let mut add = |feed_name: &str, kind: MarketDataCsvKind, priority: i64| {
        let path = root.join(format!("{flat_id}.{feed_name}.20261011.csv"));
        let source = MarketDataCsvSource::from_path(
            PathBuf::from(&path),
            kind,
            symbol,
            price_increment,
            qty_increment,
        )
        .map_err(|error| format!("open {}: {error:?}", path.display()))?;
        sequencer.add_source(path.to_string_lossy(), priority, Box::new(source));
        Ok::<(), String>(())
    };

    add("book_l2", MarketDataCsvKind::BookL2, 2)?;
    add("book_ticker", MarketDataCsvKind::BookTicker, 1)
}

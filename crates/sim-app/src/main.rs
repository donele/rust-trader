use std::path::{Path, PathBuf};

use core::book::BookMap;
use core::symbols::{Symbol, SymbolRegistry};
use core::{
    ClientId, Message, OrderAction, OrderId, OrderRequest, OrderType, ParentOrderId, Side,
    StrategyId, SymbolId, TimeInForce,
};
use feed::{MarketDataCsvKind, MarketDataCsvSource, SimSequencer};
use sim_exchange::ExchangeSimulator;
use strategy::StrategyHost;

fn main() {
    if let Some(root) = std::env::var_os("FERRIS_CSV_ROOT") {
        if let Err(error) = run_ferris_day(Path::new(&root)) {
            eprintln!("ferris run failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    let events = vec![Message::OrderRequest(OrderRequest {
        ts_nanos: 0,
        symbol: SymbolId(1),
        parent_order_id: ParentOrderId(0),
        client_id: ClientId(1),
        order_id: OrderId(0),
        action: OrderAction::New,
        side: Side::Buy,
        price: 100.0,
        qty: 1.0,
        time_in_force: TimeInForce::Gtc,
        order_type: OrderType::Limit,
        strategy_id: StrategyId(2),
        exchange_time: 0,
        window: 0,
        post_only: false,
    })];

    let sequencer = SimSequencer::new(events);
    let mut simulator = ExchangeSimulator::new();
    let mut strategy = StrategyHost::new(StrategyId(2));

    for event in sequencer.drain() {
        if let Message::OrderRequest(request) = event {
            let update = simulator.submit(request);
            strategy.on_order_update(update);
        } else {
            let _ = strategy.on_event(&event);
        }
    }

    println!("trade_sim scaffold ready");
    println!("order_updates_seen={}", strategy.order_updates_seen());
    println!("next_step=set FERRIS_CSV_ROOT to run historical market data");
}

fn run_ferris_day(root: &Path) -> Result<(), String> {
    let mut registry = SymbolRegistry::default();
    registry
        .register(Symbol::new(
            SymbolId(1),
            "SOL__USDT__PERP__OKX",
            "OKX",
            0.01,
            0.01,
        ))
        .map_err(|error| format!("register SOL: {error:?}"))?;
    registry
        .register(Symbol::new(
            SymbolId(2),
            "ADA__USDT__PERP__OKX",
            "OKX",
            0.0001,
            0.1,
        ))
        .map_err(|error| format!("register ADA: {error:?}"))?;

    let mut sequencer = SimSequencer::default();
    add_ferris_sources(
        &mut sequencer,
        root,
        "SOL__USDT__PERP__OKX",
        SymbolId(1),
        0.01,
        0.01,
    )?;
    add_ferris_sources(
        &mut sequencer,
        root,
        "ADA__USDT__PERP__OKX",
        SymbolId(2),
        0.0001,
        0.1,
    )?;

    let mut books = BookMap::from_registry(&registry);
    let mut message_count = 0usize;
    while let Some(message) = sequencer.next_message() {
        books
            .process(&message)
            .map_err(|error| format!("process message: {error:?}"))?;
        message_count += 1;
    }

    println!("ferris csv run complete");
    println!("messages_processed={message_count}");
    for symbol_id in [SymbolId(1), SymbolId(2)] {
        let book = books
            .get(symbol_id)
            .ok_or_else(|| format!("missing book for {symbol_id:?}"))?;
        let bid = book.best_bid().map(|level| level.price.to_value());
        let ask = book.best_ask().map(|level| level.price.to_value());
        println!("{symbol_id:?}: best_bid={bid:?} best_ask={ask:?}");
    }
    Ok(())
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

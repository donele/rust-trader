use std::path::Path;

use core::symbols::{Symbol, SymbolRegistry};
use core::{StrategyId, SymbolId};
use feed::ferris_sources_for_registry;

mod simulation_sequencer;

use simulation_sequencer::SimulationSequencer;

const DEFAULT_DATA_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/Ferris_csv/XYZX/2026/1011"
);

fn main() -> Result<(), String> {
    let root = Path::new(DEFAULT_DATA_ROOT);
    let registry = symbol_registry()?;
    let mut sequencer = SimulationSequencer::new(&registry, StrategyId(2));
    let sources = ferris_sources_for_registry(root, &registry, "20261011")?;
    sequencer.add_sources(sources);
    sequencer.run()?;

    println!("trade_sim complete");
    println!("data_root={}", root.display());
    println!("source_messages={}", sequencer.source_messages());
    println!("events_processed={}", sequencer.events_processed());
    println!("order_updates_seen={}", sequencer.order_updates_seen());
    for symbol_id in [SymbolId(1), SymbolId(2)] {
        let book = sequencer
            .book(symbol_id)
            .ok_or_else(|| format!("missing book for {symbol_id:?}"))?;
        let bid = book.best_bid().map(|level| level.price.to_value());
        let ask = book.best_ask().map(|level| level.price.to_value());
        println!("{symbol_id:?}: best_bid={bid:?} best_ask={ask:?}");
    }
    Ok(())
}

fn symbol_registry() -> Result<SymbolRegistry, String> {
    SymbolRegistry::from_symbols([
        Symbol::new(SymbolId(1), "ABC__XYZX", "XYZX", 0.01, 0.01),
        Symbol::new(SymbolId(2), "DEF__XYZX", "XYZX", 0.0001, 0.1),
    ])
    .map_err(|error| format!("create symbol registry: {error:?}"))
}

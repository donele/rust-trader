use std::path::Path;

use core::symbols::SymbolRegistry;
use core::SymbolId;

use crate::{MarketDataCsvError, MarketDataCsvKind, MarketDataCsvSource, MessageSource};

/// A source together with the metadata needed by a sequencer to install it.
pub struct SourceSpec {
    pub name: String,
    pub priority: i64,
    pub source: Box<dyn MessageSource>,
}

/// Builds the L2 and book-ticker sources for one symbol's Ferris CSV files.
pub fn ferris_sources(
    root: impl AsRef<Path>,
    flat_id: &str,
    date: &str,
    symbol: SymbolId,
    price_increment: f64,
    qty_increment: f64,
) -> Result<Vec<SourceSpec>, String> {
    let root = root.as_ref();
    let mut sources = Vec::with_capacity(2);

    for (feed_name, kind, priority) in [
        ("book_l2", MarketDataCsvKind::BookL2, 2),
        ("book_ticker", MarketDataCsvKind::BookTicker, 1),
    ] {
        let path = root.join(format!("{flat_id}.{feed_name}.{date}.csv"));
        let source =
            MarketDataCsvSource::from_path(&path, kind, symbol, price_increment, qty_increment)
                .map_err(|error: MarketDataCsvError| {
                    format!("open {}: {error:?}", path.display())
                })?;
        sources.push(SourceSpec {
            name: path.to_string_lossy().into_owned(),
            priority,
            source: Box::new(source),
        });
    }

    Ok(sources)
}

/// Builds all Ferris CSV sources for the symbols in a registry.
pub fn ferris_sources_for_registry(
    root: impl AsRef<Path>,
    registry: &SymbolRegistry,
    date: &str,
) -> Result<Vec<SourceSpec>, String> {
    let mut symbols: Vec<_> = registry.iter().collect();
    symbols.sort_by_key(|symbol| symbol.id);

    let mut sources = Vec::with_capacity(symbols.len() * 2);
    for symbol in symbols {
        sources.extend(ferris_sources(
            root.as_ref(),
            &symbol.flat_id,
            date,
            symbol.id,
            symbol.price_increment,
            symbol.qty_increment,
        )?);
    }
    Ok(sources)
}

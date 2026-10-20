use std::collections::HashMap;

use crate::SymbolId;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub id: SymbolId,
    pub flat_id: String,
    pub exchange: String,
    pub price_increment: f64,
    pub qty_increment: f64,
    pub min_trade_qty: f64,
    pub min_trade_notional: f64,
    pub contract_multiplier: f64,
}

impl Symbol {
    pub fn new(
        id: SymbolId,
        flat_id: impl Into<String>,
        exchange: impl Into<String>,
        price_increment: f64,
        qty_increment: f64,
    ) -> Self {
        Self {
            id,
            flat_id: flat_id.into(),
            exchange: exchange.into(),
            price_increment,
            qty_increment,
            min_trade_qty: 0.0,
            min_trade_notional: 0.0,
            contract_multiplier: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolRegistryError {
    DuplicateId(SymbolId),
    DuplicateFlatId(String),
    InvalidPriceIncrement,
    InvalidQtyIncrement,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolRegistry {
    by_id: HashMap<SymbolId, Symbol>,
    id_by_flat_id: HashMap<String, SymbolId>,
}

impl SymbolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_symbols(
        symbols: impl IntoIterator<Item = Symbol>,
    ) -> Result<Self, SymbolRegistryError> {
        let mut registry = Self::new();
        for symbol in symbols {
            registry.register(symbol)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, symbol: Symbol) -> Result<(), SymbolRegistryError> {
        if symbol.price_increment <= 0.0 || !symbol.price_increment.is_finite() {
            return Err(SymbolRegistryError::InvalidPriceIncrement);
        }
        if symbol.qty_increment <= 0.0 || !symbol.qty_increment.is_finite() {
            return Err(SymbolRegistryError::InvalidQtyIncrement);
        }
        if self.by_id.contains_key(&symbol.id) {
            return Err(SymbolRegistryError::DuplicateId(symbol.id));
        }
        if self.id_by_flat_id.contains_key(&symbol.flat_id) {
            return Err(SymbolRegistryError::DuplicateFlatId(symbol.flat_id));
        }

        self.id_by_flat_id.insert(symbol.flat_id.clone(), symbol.id);
        self.by_id.insert(symbol.id, symbol);
        Ok(())
    }

    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.by_id.get(&id)
    }

    pub fn get_by_flat_id(&self, flat_id: &str) -> Option<&Symbol> {
        self.id_by_flat_id
            .get(flat_id)
            .and_then(|id| self.by_id.get(id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.by_id.values()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_indexes_symbols_by_id_and_flat_id() {
        let mut registry = SymbolRegistry::default();
        registry
            .register(Symbol::new(SymbolId(1), "BTC-USDT", "BINANCE", 0.1, 0.001))
            .unwrap();

        assert_eq!(registry.get(SymbolId(1)).unwrap().flat_id, "BTC-USDT");
        assert_eq!(registry.get_by_flat_id("BTC-USDT").unwrap().id, SymbolId(1));
    }

    #[test]
    fn registry_rejects_duplicate_symbols() {
        let mut registry = SymbolRegistry::default();
        let symbol = Symbol::new(SymbolId(1), "BTC-USDT", "BINANCE", 0.1, 0.001);
        registry.register(symbol.clone()).unwrap();

        assert_eq!(
            registry.register(symbol),
            Err(SymbolRegistryError::DuplicateId(SymbolId(1)))
        );
    }
}

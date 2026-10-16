use core::{
    AlgoOrderRequest, BookTicker, ClockMessage, EndTimeMessage, FundingRate, Incremental,
    Liquidation, Message, OrderUpdate, PacketBegin, PacketEnd, Reset, StrategyId, Trade,
};
use orders::{OrderManager, PositionManager, RiskManager};

use crate::{Dispatcher, FarPegStrategy, Strategy};

#[derive(Debug)]
pub struct StrategyHost {
    order_manager: OrderManager,
    position_manager: PositionManager,
    risk_manager: RiskManager,
    strategy: Box<dyn Strategy>,
}

impl StrategyHost {
    pub fn new(strategy_id: StrategyId) -> Self {
        Self::with_strategy(strategy_id, Box::new(FarPegStrategy::new(strategy_id)))
    }

    pub fn with_strategy(_strategy_id: StrategyId, strategy: Box<dyn Strategy>) -> Self {
        Self {
            order_manager: OrderManager::new(),
            position_manager: PositionManager::new(),
            risk_manager: RiskManager::new(),
            strategy,
        }
    }

    pub fn on_event(&mut self, message: &Message) {
        let _ = &self.risk_manager;
        Dispatcher::route(self, message)
    }

    pub fn on_incremental(&mut self, value: &Incremental) {
        self.strategy.on_incremental(value);
    }

    pub fn on_book_ticker(&mut self, value: &BookTicker) {
        self.strategy.on_book_ticker(value);
    }

    pub fn on_trade(&mut self, value: &Trade) {
        self.strategy.on_trade(value);
    }

    pub fn on_funding_rate(&mut self, value: &FundingRate) {
        self.strategy.on_funding_rate(value);
    }

    pub fn on_liquidation(&mut self, value: &Liquidation) {
        self.strategy.on_liquidation(value);
    }

    pub fn on_clock(&mut self, value: &ClockMessage) {
        self.strategy.on_clock(value);
    }

    pub fn on_end_time(&mut self, value: &EndTimeMessage) {
        self.strategy.on_end_time(value);
    }

    pub fn on_packet_begin(&mut self, value: &PacketBegin) {
        self.strategy.on_packet_begin(value);
    }

    pub fn on_packet_end(&mut self, value: &PacketEnd) {
        self.strategy.on_packet_end(value);
    }

    pub fn on_reset(&mut self, value: &Reset) {
        self.strategy.on_reset(value);
    }

    pub fn on_algo_order_request(&mut self, value: &AlgoOrderRequest) {
        self.strategy.on_algo_order_request(value);
    }

    pub fn on_order_update(&mut self, update: &OrderUpdate) {
        self.order_manager.on_update(*update);
        self.position_manager.on_order_update(update);
    }

    pub fn order_updates_seen(&self) -> usize {
        self.order_manager.updates_seen()
    }
}

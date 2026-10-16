use core::{
    AlgoOrderRequest, BookTicker, ClockMessage, EndTimeMessage, FundingRate, Incremental,
    Liquidation, PacketBegin, PacketEnd, Reset, StrategyId, Trade,
};

pub trait Strategy: std::fmt::Debug {
    fn on_incremental(&mut self, _: &Incremental) {}

    fn on_book_ticker(&mut self, _: &BookTicker) {}

    fn on_trade(&mut self, _: &Trade) {}

    fn on_funding_rate(&mut self, _: &FundingRate) {}

    fn on_liquidation(&mut self, _: &Liquidation) {}

    fn on_clock(&mut self, _: &ClockMessage) {}

    fn on_end_time(&mut self, _: &EndTimeMessage) {}

    fn on_packet_begin(&mut self, _: &PacketBegin) {}

    fn on_packet_end(&mut self, _: &PacketEnd) {}

    fn on_reset(&mut self, _: &Reset) {}

    fn on_algo_order_request(&mut self, _: &AlgoOrderRequest) {}
}

#[derive(Debug)]
pub struct FarPegStrategy {
    strategy_id: StrategyId,
}

impl FarPegStrategy {
    pub fn new(strategy_id: StrategyId) -> Self {
        Self { strategy_id }
    }

    pub fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }
}

impl Strategy for FarPegStrategy {}

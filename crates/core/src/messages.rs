use super::*;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    PacketBegin = 1,
    PacketEnd = 2,
    IncrementalL2 = 4,
    BookTicker = 5,
    Trade = 6,
    FundingRate = 7,
    Liquidation = 8,
    AlgoOrderRequest = 10,
    OrderRequest = 11,
    OrderUpdate = 12,
    OpenOrderUpdate = 13,
    Reset = 15,
    Clock = 16,
    EndTime = 17,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incremental {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub price: f64,
    pub qty: f64,
    pub num_orders_at_level: u64,
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BookTicker {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub best_bid_price: f64,
    pub best_bid_qty: f64,
    pub best_ask_price: f64,
    pub best_ask_qty: f64,
    pub update_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trade {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub price: f64,
    pub qty: f64,
    pub trade_id: TradeId,
    pub side: Side,
    pub is_market_maker: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FundingRate {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub funding_time: TimeNanos,
    pub funding_rate: f64,
    pub funding_rate_pred: f64,
    pub open_interest: f64,
    pub last_price: f64,
    pub index_price: f64,
    pub mark_price: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Liquidation {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub price: f64,
    pub qty: f64,
    pub trade_id: TradeId,
    pub side: Side,
    pub is_market_maker: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderRequest {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub parent_order_id: ParentOrderId,
    pub client_id: ClientId,
    pub order_id: OrderId,
    pub action: OrderAction,
    pub side: Side,
    pub price: f64,
    pub qty: f64,
    pub time_in_force: TimeInForce,
    pub order_type: OrderType,
    pub strategy_id: StrategyId,
    pub exchange_time: TimeNanos,
    pub window: u64,
    pub post_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderUpdate {
    pub ts_nanos: TimeNanos,
    pub symbol: SymbolId,
    pub parent_order_id: ParentOrderId,
    pub client_id: ClientId,
    pub order_id: OrderId,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub post_only: bool,
    pub order_price: f64,
    pub order_qty: f64,
    pub filled_qty: f64,
    pub cum_qty: f64,
    pub open_qty: f64,
    pub executed_price: f64,
    pub average_fill_price: f64,
    pub fees: f64,
    pub trade_id: TradeId,
    pub exchange_time: TimeNanos,
    pub strategy_id: StrategyId,
    pub order_status: OrderStatus,
    pub reject_reason: RejectReason,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlgoOrderRequest {
    pub client_id: ClientId,
    pub parent_order_id: ParentOrderId,
    pub symbol: SymbolId,
    pub side: Side,
    pub qty: f64,
    pub start_time: TimeNanos,
    pub end_time: TimeNanos,
    pub strategy_type: StrategyType,
    pub action: OrderRequestAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockMessage {
    pub ts_nanos: TimeNanos,
    pub end_of_day: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndTimeMessage {
    pub ts_nanos: TimeNanos,
    pub end_of_day: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketBegin {
    pub symbol: SymbolId,
    pub ts_nanos: TimeNanos,
    pub snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketEnd {
    pub symbol: SymbolId,
    pub ts_nanos: TimeNanos,
    pub snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reset {
    pub symbol: SymbolId,
    pub reason: ResetReason,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Incremental(Incremental),
    BookTicker(BookTicker),
    Trade(Trade),
    FundingRate(FundingRate),
    Liquidation(Liquidation),
    OrderUpdate(OrderUpdate),
    AlgoOrderRequest(AlgoOrderRequest),
    Clock(ClockMessage),
    EndTime(EndTimeMessage),
    PacketBegin(PacketBegin),
    PacketEnd(PacketEnd),
    Reset(Reset),
}

impl Message {
    pub fn ts_nanos(&self) -> TimeNanos {
        match self {
            Self::Incremental(value) => value.ts_nanos,
            Self::BookTicker(value) => value.ts_nanos,
            Self::Trade(value) => value.ts_nanos,
            Self::FundingRate(value) => value.ts_nanos,
            Self::Liquidation(value) => value.ts_nanos,
            Self::OrderUpdate(value) => value.ts_nanos,
            Self::AlgoOrderRequest(value) => value.start_time,
            Self::Clock(value) => value.ts_nanos,
            Self::EndTime(value) => value.ts_nanos,
            Self::PacketBegin(value) => value.ts_nanos,
            Self::PacketEnd(value) => value.ts_nanos,
            Self::Reset(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_expose_their_message_time() {
        let message = Message::Trade(Trade {
            ts_nanos: 123,
            symbol: SymbolId(7),
            price: 100.5,
            qty: 2.0,
            trade_id: 9,
            side: Side::Buy,
            is_market_maker: false,
        });
        assert_eq!(message.ts_nanos(), 123);
    }

    #[test]
    fn message_type_values_match_reference_protocol() {
        assert_eq!(MessageType::Trade as u16, 6);
        assert_eq!(MessageType::OrderRequest as u16, 11);
        assert_eq!(MessageType::OrderUpdate as u16, 12);
        assert_eq!(MessageType::EndTime as u16, 17);
    }
}

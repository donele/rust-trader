use core::{Message, OrderId, OrderRequest, OrderUpdate};

#[derive(Debug, Default)]
pub struct ExchangeSimulator {
    next_order_id: u64,
}

impl ExchangeSimulator {
    pub fn new() -> Self {
        Self { next_order_id: 1 }
    }

    /// For now, market-data latency is zero: the exchange forwards the event
    /// to the client path immediately after receiving it.
    pub fn on_market_data(&mut self, message: Message) -> Message {
        message
    }

    pub fn submit(&mut self, request: OrderRequest) -> OrderUpdate {
        let order_id = OrderId(self.next_order_id);
        self.next_order_id += 1;

        OrderUpdate {
            ts_nanos: request.ts_nanos,
            symbol: request.symbol,
            parent_order_id: request.parent_order_id,
            client_id: request.client_id,
            order_id,
            side: request.side,
            order_type: request.order_type,
            time_in_force: request.time_in_force,
            post_only: request.post_only,
            order_price: request.price,
            order_qty: request.qty,
            filled_qty: 0.0,
            cum_qty: 0.0,
            open_qty: request.qty,
            executed_price: 0.0,
            average_fill_price: 0.0,
            fees: 0.0,
            trade_id: -1,
            exchange_time: request.exchange_time,
            strategy_id: request.strategy_id,
            order_status: core::OrderStatus::New,
            reject_reason: core::RejectReason::None,
        }
    }
}

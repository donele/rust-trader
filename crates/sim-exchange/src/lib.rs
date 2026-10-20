use core::{Message, OrderId, OrderRequest, OrderUpdate};

pub trait ExchangeMessageSink {
    fn enqueue(&mut self, message: Message, strategy_time: core::TimeNanos, priority: i64);
}

#[derive(Debug, Default)]
pub struct ExchangeSimulator {
    next_order_id: u64,
}

impl ExchangeSimulator {
    pub fn new() -> Self {
        Self { next_order_id: 1 }
    }

    pub fn process_message(&mut self, message: &Message) {
        if matches!(message, Message::OrderUpdate(_)) {
            return;
        }

        // Market-data processing and fill simulation will be added here.
    }

    pub fn submit<S: ExchangeMessageSink>(&mut self, request: OrderRequest, sink: &mut S) {
        let order_id = OrderId(self.next_order_id);
        self.next_order_id += 1;

        let update = OrderUpdate {
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
        };
        sink.enqueue(Message::OrderUpdate(update), update.ts_nanos, 0);
    }
}

# Design Plan

## Goal

Build a Rust simulation stack that is useful early, structurally sound, and incrementally
extensible toward broader functional coverage.

The target is a Rust simulation system that preserves the key semantics of the established
`trade_sim` runtime:

- event ordering semantics
- market-data replay semantics
- exchange simulation behavior
- order / position / risk state transitions
- strategy dispatch flow

while adopting Rust-native structure instead of mechanically copying legacy class shapes.

## Reference architecture

The current target runtime flow is:

`SimSequencer -> ExchangeSimulator -> Dispatcher -> StrategyHost -> Strategy`

Within that flow, the strategy/services layer is responsible for:

- book updates
- order state updates
- position updates
- risk checks
- metric updates
- dispatch to strategy logic

The sequencer is the higher-level simulation coordinator. It owns the ordered stream of events and
advances the simulation one event at a time. The exchange receives each market-data event first.
The exchange then forwards the event to the client path. The dispatcher owns the client-visible
book, updates it, and routes the same message to the strategy host.

The first implementation uses zero latency. The explicit exchange-to-client handoff is still
important because it establishes the correct causal path before latency and priority-queue delivery
are added.

## Runtime design principles

This section describes the architectural rules that future implementations should follow. These
rules are more important than the current module names. A future component may be renamed or split
into several files, but it should not quietly change the direction of data flow or take ownership of
state that belongs to another layer.

### Live trading is the baseline model

The live runtime is the simpler and more general model because it does not contain an exchange
simulator. External exchange gateways and market-data feeds deliver messages as they arrive. The
runtime processes those messages in one direction:

```text
LiveSequencer
    -> Dispatcher
        -> StrategyHost
            -> Strategy
                -> Trader
                    -> OrderManager
                        -> OrderSink
                            -> ExecutionGateway
```

The arrows describe ownership of the next processing step, not necessarily a synchronous function
call. A live feed may invoke the sequencer from an I/O callback, and an execution gateway may later
deliver an acknowledgement through another ingress path. The logical ownership and direction remain
the same.

The runtime should be designed as a flat processing pipeline:

1. The sequencer receives one external message.
2. The dispatcher updates client-visible market state and routes the message by its type.
3. The strategy host updates order, position, and risk state as appropriate.
4. The strategy evaluates the event.
5. If the strategy decides to trade, the downstream order path creates an exchange-bound order.
6. The order sink sends that order to the configured execution gateway.
7. Later exchange responses enter again through the normal ingress path.

Each stage should do its own work and hand off explicitly. Avoid hidden calls that jump from a
downstream stage back into an upstream stage. In particular:

- the strategy should not call the dispatcher directly
- the dispatcher should not submit orders to an exchange gateway
- the order sink should not update the client book
- the execution gateway should not call strategy methods directly
- a response should re-enter through the sequencer/ingress path rather than recursively invoking
  the component that originally created the order

This flat structure makes the live runtime easier to test and gives simulation a clean place to
insert scheduling and latency without changing the shared strategy path.

### Responsibilities of the live components

#### Live sequencer

The live sequencer is an ingress and ordering boundary. It receives market data, control messages,
customer algorithm requests, and execution responses, then passes them into the client processing
pipeline as they arrive.

It should not own strategy state, order state, or exchange simulation state. It should not decide
whether a market-data message is a buy or sell signal. It only preserves the input stream's ordering
and invokes the next client-side stage.

The live sequencer does not need a simulation priority queue. Network delivery and exchange
delivery already determine when live messages arrive. If a live implementation needs buffering for
technical reasons, that buffering is an ingress concern and must not become a simulation-time model.

#### Dispatcher

The dispatcher is the client-side message router. It owns or coordinates the client-visible
`BookMap` and applies market-data updates before the strategy observes them.

Its responsibilities are:

- update the client-visible market-data state
- route each message type to the appropriate typed handler
- deliver `AlgoOrderRequest` to the strategy host
- deliver `OrderUpdate` to order, position, and risk state
- forward control and lifecycle events to the correct downstream handler

The dispatcher should not create exchange-bound orders, submit orders to a gateway, or model
network/exchange latency. It should not need to know whether it is running in live trading or
simulation.

#### Strategy host

The strategy host owns the client-side services surrounding a strategy:

- the strategy instance
- order manager
- position manager
- risk manager
- strategy-specific service context

The host translates typed routed events into strategy callbacks and keeps state-manager updates
centralized. Strategy callbacks are notifications, not an order-return protocol. Exchange-bound
orders should be created by the downstream trader/order-manager path and sent through an order sink.

#### Strategy

The strategy consumes client-visible events and customer algorithm requests. It may update its own
state and request trading decisions through the downstream trading interfaces, but it should not
know whether the eventual sink is live or simulated.

The strategy should not receive raw simulation queue entries, delivery timestamps, exchange targets,
or simulator-only state. If a strategy needs time, it should receive a normal clock or event message
through the shared message model.

#### Trader and order manager

The trader turns strategy decisions into exchange-bound order intent. The order manager owns order
lifecycle state and applies risk checks and state transitions. These components are downstream of
the strategy and upstream of the order sink.

An exchange-bound `OrderRequest` is a direct value for the downstream order path. It is not an
incoming client message and should not be routed through the dispatcher or strategy callbacks.

#### Order sink and execution gateway

The order sink is the polymorphic boundary between the shared order path and the execution
environment:

- a live sink forwards orders to an execution gateway
- a simulated sink forwards orders to the exchange simulator

The execution gateway is responsible for exchange-specific protocols and transport. It should not
own strategy state or client market-data state.

### Message direction and ownership

Message type and processing direction are related but not identical. The following rules apply to
the shared runtime:

| Value or message | Origin | Normal destination | Notes |
|---|---|---|---|
| market-data message | feed or exchange gateway | dispatcher/client path | Updates the client-visible book before strategy processing. |
| `AlgoOrderRequest` | customer or external algorithm source | strategy host | The strategy uses it to create downstream trading intent. |
| `OrderRequest` | trader/order manager | order sink/exchange | Direct exchange-bound value; not a dispatcher message. |
| `OrderUpdate` | execution gateway or exchange simulator | dispatcher/client path | Updates order and position state and is then passed downstream. |
| control/clock message | runtime or external control source | dispatcher/client path | Must use the normal typed routing path. |

`OrderRequest` and `OrderUpdate` are intentionally asymmetric. An order request travels downstream
toward an execution environment. An order update travels upstream into the client runtime. The
dispatcher may process the update, but it must never turn a routed update into a returned order.

### No hidden simulation concepts in shared code

Shared client-side components should be usable by both live and simulated runtimes without seeing
simulation-only concepts. In particular, the following belong only to the simulation coordinator:

- simulated delivery timestamps
- simulation priority values
- simulation queue entries
- exchange/client event targets
- simulated latency configuration
- exchange fill-model state

The live sequencer should not need to construct a simulated event target or a simulated scheduled
event. The dispatcher should receive a normal message in both modes.

## Exchange simulator integration

The exchange simulator is inserted between the simulation sequencer and the shared client pipeline.
It is a simulation-specific component and may therefore understand simulation time, fill rules,
resting client orders, and depth simulation data.

The simulation flow is:

```text
SimulationSequencer
    -> ExchangeSimulator
    -> Dispatcher
        -> StrategyHost
            -> Strategy
                -> Trader / OrderManager
                    -> simulated OrderSink
                        -> ExchangeSimulator
```

The simulation sequencer owns the simulation priority queue. `run_ferris_data` or another data
loader should only register sources and start the sequencer. It should not inspect message variants,
call the exchange directly, call the dispatcher directly, or handle order updates specially.

### Exchange-first processing

For every message taken from the simulation queue, the exchange simulator gets the first chance to
process it. This preserves the causal rule that the exchange observes market data before the client
does.

The exchange handles messages by their shared type:

| Message | Exchange simulator action | Client dispatcher action |
|---|---|---|
| market data | Use it for fill/depth simulation and any exchange-side state | Update client book and notify strategy when the client receives it. |
| `AlgoOrderRequest` | Ignore; this is client-side strategy input | Pass to strategy host. |
| `OrderRequest` | Consume through the simulated order sink | Never route through dispatcher. |
| `OrderUpdate` | Ignore; the exchange already generated it | Pass to order/position/strategy state. |
| clock/control/reset | Apply only if relevant to exchange state | Pass through normal client routing when applicable. |

The exchange simulator must not process an `OrderUpdate` as a newly received order response or
attempt to generate another update from it. The simulation sequencer can send the update through the
same ordinary message path; message-type handling prevents it from re-entering exchange processing
as a new exchange action.

The dispatcher likewise should not special-case simulation-generated updates. It handles an
`OrderUpdate` exactly as it handles a live execution response. This is one of the most important
live/simulation parity rules.

### Simulation priority queue

The simulation priority queue is owned by `SimulationSequencer`, not by the shared dispatcher or
strategy host. It merges:

- market-data source messages
- customer algorithm-order messages
- exchange-generated order updates
- other simulation-generated control or clock messages
- future client-to-exchange scheduled work when order latency is modeled

The queue orders entries by:

1. delivery time
2. configured priority
3. monotonic insertion sequence

The insertion sequence is required for deterministic behavior when two events have equal delivery
times and priorities.

The queue may use internal simulation-only metadata to distinguish an exchange-generated event from
a source event, but that metadata must remain inside the simulation sequencer. It must not appear in
the shared `Message` type or leak into the live sequencer and dispatcher APIs.

At the application level, the desired code should remain simple:

```rust
let mut sequencer = SimulationSequencer::new();
add_sources(&mut sequencer, root)?;
sequencer.run()?;
```

The message discrimination and queue reinsertion happen inside the simulation sequencer. This
keeps application wiring declarative and prevents future data loaders from duplicating runtime
rules.

### Latency model

Latency should be modeled as delivery scheduling, not by changing the message's original timestamp.
At minimum, simulation may eventually need separate latency values for:

- market data from exchange to client
- new orders from client to exchange
- cancels and modifications from client to exchange
- acknowledgements from exchange to client
- fills from exchange to client

The message retains its event or exchange timestamp. The simulation queue adds a delivery time that
controls when the recipient may process it:

```rust
struct ScheduledMessage {
    event_time: TimeNanos,
    delivery_time: TimeNanos,
    priority: i64,
    sequence: u64,
    message: Message,
}
```

The current phase uses zero latency. Zero latency means that the exchange processes an event first
and the client may process the resulting delivery immediately afterward. It does not mean that the
exchange is bypassed or that the strategy is called directly from the source.

When a client-facing message is generated by the exchange, the exchange should send it through a
client-message sink. The sink is currently allowed to be a no-op while the client-delivery path is
being built. The important rule is that the exchange should not return an ad hoc `client_message`
to application code for manual handling. Once the sink is implemented, it can enqueue the message
with the appropriate delivery time without changing the application loop.

### Exchange and client order books

The exchange and client should not accidentally share one mutable book when latency is modeled.
There are two valid strategies:

1. Keep one client-visible `BookMap` and maintain separate exchange-side fill/depth state. This is
   appropriate when the exchange simulator consumes raw market data and uses a specialized fill
   model rather than reconstructing a full exchange book.
2. Maintain an exchange book and a client book. This is appropriate when fills require full-depth
   exchange state and the client must observe a delayed projection.

Two clocks alone do not make one mutable book represent two simultaneous views. A shared book would
show the exchange's latest update to the client unless it supports versioned or historical views.
That advanced versioned design is possible, but it is logically multiple views and should not be
introduced accidentally.

The current design selects the first option: the dispatcher owns the client-visible `BookMap`, and
the exchange simulator owns its order list and future fill/depth state. If a full exchange book is
needed later, it should be added inside the exchange simulator without moving the client book into
the dispatcher or strategy host.

### Simulation/live parity rules

The following rules should guide future changes:

- live and simulation strategies receive the same shared message shapes
- live and simulation dispatchers use the same typed routing behavior
- only the sequencer implementation changes between live and simulation
- only the order sink and execution environment change between live and simulation
- simulation latency belongs to the simulation sequencer and exchange simulator
- client order and position state belongs to the shared client runtime
- exchange order/fill/depth state belongs to the exchange implementation
- no application-level loop should branch on every message type merely to keep the runtime working

If a new feature requires a simulation-only field in `Message`, first check whether it belongs in a
scheduled queue entry or exchange-simulator state instead. Shared messages should describe domain
events, not the mechanics used to deliver them in one runtime mode.

## Exchange/client market-data state and timing

### Why this decision matters

The exchange and the trading client do not observe market data in the same way. The exchange sees
an incoming market-data event at exchange time. The client sees a delivered version of that event
after market-data latency. Similarly, an order is created by the client, arrives at the exchange
after order latency, and produces an acknowledgement or fill that arrives back at the client after
another delay.

This distinction matters even when all latencies are currently zero. If the event path is designed
as `sequencer -> client book -> strategy`, it becomes easy for the client to observe market data
before the exchange has processed it. That reverses the intended causality and makes later fill and
latency work harder.

The intended causal flow is:

```text
market-data source
    -> SimSequencer
        -> ExchangeSimulator
            -> client market-data delivery
                -> Dispatcher
                    -> client BookMap
                    -> StrategyHost
                        -> Strategy
```

For an order, the direction reverses:

```text
Strategy
    -> OrderManager / OrderSink
        -> SimSequencer
            -> ExchangeSimulator
                -> acknowledgement or fill
                    -> SimSequencer
                        -> Dispatcher / StrategyHost
```

The exchange owns client-order state and the depth/fill simulation data it needs to make fill
decisions. The client book is not used by the exchange to decide fills.

### One book versus two books

There are two reasonable ways to represent market-data state.

#### One client book plus exchange-side fill state

In this model, the client has one `BookMap`. The exchange consumes the raw market-data stream and
keeps the order list and depth-simulation state required by its fill model, but it does not update
or query the client's `BookMap`.

```text
raw market data
    -> exchange fill model
    -> delayed or immediate client delivery
        -> client BookMap
```

Advantages:

- avoids maintaining a second full order book when the fill model does not need one
- makes the client book naturally represent what the client has actually received
- allows the exchange to use a specialized depth or queue-position model
- reduces memory and book-update work

Costs and risks:

- the exchange and client have different representations of market state
- debugging requires inspecting both the raw event stream and exchange fill state
- the exchange must never accidentally use the delayed client book for fill decisions
- a depth-aware exchange may eventually need state that is logically equivalent to a second book

This is the selected model for the current implementation. The exchange simulator receives the
market-data event first, then forwards it with zero latency. `Dispatcher` owns the single client
`BookMap` and updates it before invoking the strategy host.

#### Two books plus one simulation clock

In this model, the exchange maintains an authoritative exchange book and the client maintains a
separate delayed book:

```text
raw market data -> exchange BookMap -> delayed client BookMap
```

Advantages:

- exchange and client state are explicit and independently inspectable
- full-depth matching and queue-position simulation are straightforward to model
- stale client state is represented directly
- tests can compare exchange state with client-visible state

Costs and risks:

- duplicate memory and book-building work
- duplicated reset, snapshot, and gap-handling logic
- risk of treating the client book as authoritative when only the exchange book should drive fills

This remains a valid future option. It should be added when the fill model requires a reconstructed
exchange book, rather than added merely to represent latency.

#### Why two clocks alone are not enough

Two clocks do not make one mutable book represent two views at the same time. If the exchange
updates a shared book immediately, the client can observe the new state even when its local clock is
behind.

A single physical book can support two views only if it is versioned, event-sourced, or capable of
answering historical queries such as “what was the book visible to the client at client time `t`?”
That is a valid advanced design, but it is effectively maintaining multiple logical views and is
more complicated than the current needs justify.

### Clock and event-time model

The simulation should use one deterministic global simulation timeline with explicit event and
delivery times. Separate exchange and client clock counters may be useful as implementation
details, but independent clocks are not required to model latency.

A future scheduled event should carry at least:

```rust
struct ScheduledEvent {
    event_time: TimeNanos,
    delivery_time: TimeNanos,
    sequence: u64,
    message: Message,
}
```

`event_time` records when the event occurred at its origin. `delivery_time` determines when the
recipient may process it. `sequence` provides deterministic ordering for events with equal delivery
times. The message's own timestamp should not be overwritten merely because delivery is delayed.

Latency should eventually be modeled independently for at least:

- market data from exchange to client
- new orders from client to exchange
- cancels and modifications from client to exchange
- acknowledgements and fills from exchange to client

For the current phase all of these are zero. The exchange-to-client method is nevertheless kept as
an explicit boundary so adding a priority queue later does not require redesigning the message
ownership model.

### Event ordering and causality rules

The coordinator must preserve these rules:

1. The exchange receives a market-data event before the client receives it.
2. The client book is updated before strategy code observes that market-data event.
3. A client order cannot be matched before it reaches the exchange.
4. An acknowledgement or fill cannot reach the client before the exchange creates it.
5. Equal-time events use a stable priority and sequence ordering.
6. Exchange fill decisions never read delayed client-visible state.

These rules should remain true when latency is zero. Zero latency means “deliver immediately after
the producing component returns,” not “bypass the exchange or call the strategy directly.”

## Version 1 scope

The first version focuses on a reliable simulation engine with a clear vertical slice.

Version 1 covers:

- core market and order event modeling
- deterministic replay and sequencing
- exchange simulation
- order, position, and risk state
- strategy host and a simplified `FarPegStrategy`
- one or two trader implementations
- configuration loading suitable for local simulation runs

Version 1 does not need:

- live shared-memory playback
- production order placer integration
- full alpha graph coverage
- highly optimized IPC infrastructure

## Recommended repository shape

Use a Cargo workspace from the start. The domain is large enough that one flat crate will become
hard to maintain quickly.

Suggested top-level crates:

- `crates/core`
  - message model
  - time model
  - symbol model
  - book model
  - shared identifiers and enums
- `crates/feed`
  - historical sources
  - CSV order request source
  - event merge / replay support
- `crates/sim-exchange`
  - exchange simulator
  - simulated order lifecycle
  - simulated fills
- `crates/orders`
  - order manager
  - position manager
  - risk manager
  - order state caches
- `crates/strategy`
  - strategy host
  - dispatcher
  - strategies
  - strategy evaluation
  - trader interfaces
- `crates/sim-app`
  - `trade_sim` CLI entrypoint
  - config loading
  - runtime wiring

If the repo starts smaller, begin with a single crate and module boundaries that map to this
layout. The important part is preserving separable ownership boundaries so crates can be split out
without a rewrite.

## Current status

The workspace and shared message-model phase are now in place.

Completed:

- neutral shared crates: `core`, `feed`, `orders`, and `strategy`
- simulator-specific crates: `sim-exchange` and `sim-app`
- shared time alias: `TimeNanos`
- strongly typed identifiers including `SymbolId`, `OrderId`, `ClientId`, `ParentOrderId`, and `StrategyId`
- shared enums for sides, order actions, order types, time-in-force, statuses, rejection reasons, and execution strategy types
- shared market-data messages: `Incremental`, `BookTicker`, `Trade`, `FundingRate`, and `Liquidation`
- shared order and lifecycle messages: `OrderUpdate`, `AlgoOrderRequest`, `ClockMessage`, `EndTimeMessage`, `PacketBegin`, `PacketEnd`, and `Reset`
- direct exchange-bound `OrderRequest` value used between the order sink and exchange simulator
- top-level `Message` enum with explicit message timestamps
- initial tests for message timestamps and reference protocol message IDs
- workspace validation with `cargo check --workspace` and `cargo test --workspace`
- fixed-point-style `Price`/`Quantity` values for deterministic book ordering
- `Symbol` and `SymbolRegistry` with ID and flat-symbol lookup
- per-symbol `OrderBook` and `BookMap` primitives
- absolute L2 incremental updates, top-of-book updates, snapshot resets, and explicit reset handling
- basic `PositionManager` with average-price and realized-PnL accounting
- basic `RiskManager` with order-size and projected-position limits
- unit tests for symbol lookup, tick ordering, level insertion/removal, snapshot clearing, positions, and risk checks
- `MessageSource` channel abstraction and in-memory message source
- deterministic `SimSequencer` heap ordered by timestamp, channel priority, and channel insertion order
- half-open replay time-window filtering
- CSV algorithm-order source with symbol lookup, validation, sorting, and `AlgoOrderRequest` emission
- tests for multi-channel merge ordering, tie-breaking, time windows, and CSV input
- streaming Ferris CSV sources for L2 and book-ticker data
- snapshot packet-boundary emission and source-specific parsing errors
- end-to-end feed, sequencer, and `BookMap` processing tests
- full-day ABC and DEF validation from converted Ferris CSV data
- exchange-first, zero-latency market-data handoff to the client path
- dispatcher-owned client-visible `BookMap`

Still intentionally incomplete in this phase:

- exact packed/wire layouts and shared-memory serialization
- external feed parsing and message deserialization
- symbol registry and symbol metadata
- price/quantity fixed-point-style representation
- complete message families such as control, balance, position, and risk updates
- typed dispatcher routing with a client-owned `BookMap`
- polymorphic strategy boundary with a boxed `FarPegStrategy`

The current types are therefore a semantic Rust model, not yet a binary-compatible transport model.

## Next steps

Phases 1 and 2 are complete at the primitive level, the historical CSV feed slice is in place, and
the strategy dispatch boundary is implemented. The next work is:

1. Add `Trader`, `OrderManager`, and `OrderSink` integration.
2. Add simulated acknowledgements and basic exchange-side order/depth state.
3. Add the priority queue and non-zero market-data/order/fill latency.
4. Add simulated and live execution-sink interfaces.
5. Implement crossing/fill behavior, cancels, and delayed
   `OrderUpdate` emission.

## High-level implementation order

The ordering is deliberate because the later layers depend heavily on the earlier ones.

### Phase 1: Message and domain model

Implement:

- core message types
  - `Incremental`
  - `BookTicker`
  - `Trade`
  - `FundingRate`
  - `Liquidation`
  - `ClockMessage`
  - `PacketBegin`
  - `PacketEnd`
  - `Reset`
  - `OrderRequest`
  - `OrderUpdate`
  - `AlgoOrderRequest`
  - `EndTime`
- enums and identifiers
- time utilities
- symbol representation

Recommended Rust approach:

- use enums for tagged message families
- use newtypes for important identifiers
  - `SymbolId`
  - `OrderId`
  - `ClientId`
  - `StrategyId`
- keep hot-path fields as plain numeric types
- derive `Debug`, `Clone`, `Copy`, `Eq`, `PartialEq`, `Ord`, `PartialOrd`, and `Hash` where they
  are semantically valid

Reason for this ordering:

- everything else depends on these types
- exact data model decisions drive later API design

Deliverable:

- a stable Rust event model with unit tests for parsing, ordering, and debug/display semantics

### Phase 2: Book and state primitives

Implement:

- order book
- price level model
- per-symbol lookup structures
- book map
- position model
- basic risk state containers

Reason for this ordering:

- the sequencer and simulator feed these objects constantly
- strategy and alpha logic depend on stable book semantics

Recommended Rust approach:

- keep books mutable and explicit
- prefer predictable data structures over abstract trait-heavy designs
- isolate crossed-book cleanup and snapshot reset logic behind well-tested methods

Deliverable:

- deterministic book updates for incremental and book-ticker events
- tests covering crossed-book cleanup and snapshot transitions

### Phase 3: Historical feed and sequencer

Implement:

- source trait
- historical source adapters
- CSV order request source
- merged event scheduler
- `SimSequencer`

This is the heart of replay behavior.

Key requirements:

- preserve event-time ordering
- preserve packet-boundary semantics
- preserve replay behavior around start/end time windows
- preserve insertion of synthetic order-request events into the merged stream

Recommended Rust approach:

- use a binary heap or tournament-style merge around per-source cursors
- keep event ordering rules explicit and testable
- separate source parsing from replay scheduling

Deliverable:

- Rust sequencer that can replay market data and order request events in deterministic order

### Phase 4: Exchange simulator

Implement:

- simulated exchange order source
- simulated order submission
- simulated cancel flow
- order crossing / fill logic
- mask / latency rules
- delayed order update emission

This is the first hard domain layer.

Reason for this ordering:

- a lot of repo-specific behavior is encoded here
- it depends on current time, exchange time, and book state together
- small semantic differences will change strategy outcomes

Recommended Rust approach:

- make fill-policy decisions explicit in code and tests
- keep simulator time and market-data time distinct when necessary
- model scheduled simulator outputs as first-class events rather than hidden callbacks

Deliverable:

- Rust exchange simulator capable of driving a strategy host from replayed market data

### Phase 5: Order / position / risk pipeline

Implement:

- `OrderManager`
- `PositionManager`
- `RiskManager`
- order caches
- stale / closed / open order state transitions

This layer must be done before serious strategy work.

Key requirement:

- preserve order lifecycle transitions accurately enough that simulated fills, cancels, and risk
  checks produce consistent downstream state

Recommended Rust approach:

- keep the state machine explicit
- avoid passing mutable access to everything everywhere
- centralize lifecycle transition logic so invariants are enforceable

Deliverable:

- end-to-end simulated order state updates

### Phase 6: Strategy dispatcher and strategy host

Implemented:

- `Dispatcher` routes each shared market-data/control message to a typed host handler
- `Strategy` defines the polymorphic strategy boundary
- `StrategyHost` owns a boxed strategy and the order, position, and risk managers
- `FarPegStrategy` provides the initial strategy implementation

Remaining:

- per-symbol market-data service context
- per-message dispatch chain

This layer should preserve the architectural shape:

- pre-process message
- update book / position / risk / order state
- dispatch to strategy

Recommended Rust approach:

- use a concrete host struct that owns the state managers
- keep the dispatch interface simple
- avoid deep trait object graphs in version 1

Deliverable:

- a Rust strategy host that receives sequenced events and sees the same logical state transitions

### Phase 7: Strategy behavior and trader integration

Implement:

- strategy behavior, beginning with `FarPegStrategy`
- strategy evaluation and signal generation
- trigger evaluation
- trader dispatch

This should begin with a reduced subset:

- one target symbol
- one or two essential alpha paths
- one simple trader implementation

Do not start with the full graph of alphas and traders.

Deliverable:

- one working strategy path through the Rust simulator

### Phase 8: Broader strategy coverage

Add:

- more traders
- more alphas
- monitor alphas
- health logic
- richer metrics and state logging

This is the phase where the project becomes broader and more complete rather than merely usable.

## Recommended implementation style

### Keep the semantics, not the exact class shapes

Do not try to preserve every legacy template, inheritance, or callback pattern literally.

Prefer:

- explicit Rust structs and enums
- traits only where they represent real substitution boundaries
- immutable event values
- mutable hot-path state objects where performance matters

### Separate data model from runtime wiring

The legacy runtime mixes several concerns in one process:

- domain state
- dispatch logic
- runtime object graph wiring
- config-driven assembly

The Rust implementation should separate these more clearly.

Suggested split:

- domain objects
- services / managers
- simulator runtime
- CLI assembly

### Prefer concrete ownership over shared mutability

Rust will punish vague ownership decisions if they are deferred.

Prefer:

- one clear owner for each major subsystem
- borrowed reads where possible
- narrow mutable borrows
- message passing between subsystems through explicit method calls

Avoid early dependence on:

- pervasive `Rc<RefCell<_>>`
- pervasive `Arc<Mutex<_>>`
- self-referential structures

unless profiling or concurrency requirements force them later.

### Start synchronous unless async is clearly needed

Version 1 should be a deterministic offline simulator. That means synchronous code is the default.

Do not introduce async runtimes, channels, or task orchestration into the core simulation path
unless there is a concrete requirement. They complicate determinism and make debugging harder.

### Start with correctness before micro-optimization

Early benchmarks matter, but simulation semantic correctness matters more.

Do not optimize first:

- custom allocators
- object pools
- lock-free queues
- intrusive collections
- zero-copy parsing across the whole stack

until the replay and order-state behavior is stable.

## Data model guidance

### Message representation

Use a top-level shared message enum for events that can travel through the sequencer and client
runtime:

- `Market(MarketEvent)`
- `AlgoOrderRequest(AlgoOrderRequest)`
- `Simulator(SimEvent)`
- `Control(ControlEvent)`

`OrderRequest` is deliberately not a `Message` variant. It is an exchange-bound value created by
the downstream order path and passed directly to the order sink or exchange simulator. This keeps
it out of the dispatcher and prevents strategy/client code from accidentally treating an exchange
request as an incoming event.

`AlgoOrderRequest` has the opposite direction. It originates outside the strategy runtime, such as
from a customer or algorithm-order source, and is a normal sequenced message. The dispatcher passes
it to the strategy host, which eventually causes downstream exchange-bound orders to be created.

Within market events, use either:

- one enum with variant payloads, or
- one outer enum plus strongly typed structs per message

Either approach is fine. The important part is:

- total ordering is representable
- packet boundaries are representable
- event timestamps are explicit

### Time representation

Define one canonical simulation timestamp representation early.

Recommended options:

- integer nanoseconds from epoch, or
- integer nanoseconds from session start

Do not use `std::time::SystemTime` throughout the hot path. Convert at boundaries and keep the
runtime model numeric and comparable.

### Numeric representation

Decide early whether prices and quantities are stored as:

- fixed-point integers, or
- decimal wrappers over integers

Do not use raw `f64` for core order-state and fill accounting unless there is a strong reason.
Determinism and equality behavior are more important here than convenience.

### Symbol representation

Use interned or indexed symbols in the hot path.

Recommended approach:

- parse external symbol strings at startup
- map them to `SymbolId`
- store metadata in a `SymbolRegistry`

This keeps books, positions, and event routing compact and predictable.

## Data-flow target

The desired Rust data flow for the first meaningful version is:

1. config loads
2. symbol universe initializes
3. market-data sources open
4. order-request source opens
5. `SimSequencer` merges all events
6. `ExchangeSimulator` receives events
7. `StrategyHost` receives market and order events
8. dispatcher routes messages to the strategy host
9. strategy evaluates signals and sends simulated orders
10. exchange simulator schedules simulated acknowledgements / fills / cancels

That is the minimum useful vertical slice.

## Scope cut for version 1

A good version-1 target:

- historical `trade_sim` only
- one replay sequencer
- one exchange simulator
- one order manager / risk / position pipeline
- one strategy host
- one simplified strategy
- one or two trader implementations
- config loading from TOML, JSON, or YAML

Later phases can expand this with:

- live simulator support
- dump simulator support
- shared-memory playback support
- production order placer integration
- a broader metrics stack
- a broader alpha catalog

## Validation strategy

### Unit tests

Write tests for:

- message parsing / creation
- order-book transitions
- sequencer merge ordering
- simulator fill rules
- order-manager lifecycle transitions

### Differential scenario tests

For a fixed input dataset or scenario:

- compare event counts
- compare order submissions
- compare fills
- compare final positions
- compare final PnL / exposure summaries

Absolute byte-for-byte identity is not the initial goal; consistent event ordering and
state-transition behavior is the right target.

### Scenario tests

Build small deterministic scenarios for:

- passive fill
- aggressive cross
- partial fill
- cancel before ack
- cancel after partial fill
- repeated snapshot handling
- end-of-day handling

### Golden replay tests

Once the first vertical slice exists, create golden datasets with stable expected outputs:

- emitted order updates
- final order states
- final position states
- final risk summary

Keep these small enough to reason about by inspection.

## Main technical risks

### 1. Semantic drift in exchange simulation

The simulator contains many small decisions that affect fills and timing.

Mitigation:

- build scenario by scenario
- build targeted tests from known datasets and event traces
- document each intentional deviation from the established semantics

### 2. Order manager complexity

`OrderManager` is inherently large and stateful.

Mitigation:

- split into smaller Rust collaborators
  - order store
  - order lifecycle engine
  - order sender
  - order log service

### 3. Overusing Rust abstraction too early

It is easy to end up with too many traits, lifetimes, or shared ownership layers before the domain
has stabilized.

Mitigation:

- begin concrete
- introduce traits only at clear subsystem seams
- prefer owned data in version 1

### 4. Overcommitting to low-level optimization too early

Mitigation:

- first get the simulation semantics right
- optimize after correctness and replay stability are demonstrated

## Performance plan

The first Rust `trade_sim` should aim for predictable performance, not maximum cleverness.

Use:

- ordinary owned structs for most simulation state
- pre-sized maps and vectors where useful
- careful hotspot design around books and event dispatch
- `cargo bench` or focused microbenchmarks only after correctness is established

Only introduce:

- arenas
- object pools
- specialized allocators
- custom intrusive queues
- unsafe hot-path optimizations

when profiling shows a clear bottleneck.

This is different from optimizing IPC-heavy runtime components. `trade_sim` is broader and more
state-heavy than a pure publish/subscribe benchmark.

## Configuration plan

Version 1 should support simple local simulation runs with a small number of inputs:

- date or session identifier
- infra config
- model config
- symbol list
- market-data source paths
- optional order-request CSV path

Recommended crates:

- `clap` for CLI parsing
- `serde` plus one of `toml`, `serde_json`, or `serde_yaml` for config loading

Prefer plain deserialized config structs over dynamic map-style configuration in the Rust version.

## Initial milestone plan

### Milestone 1

- Cargo workspace scaffold
- core message model
- symbol model
- order-book model
- simple replay source

### Milestone 2

- merged-event sequencer
- order-request source
- end-to-end replay of market events

### Milestone 3

- exchange simulator
- order updates
- simulated fills

### Milestone 4

- order manager
- position manager
- risk manager
- basic strategy host

### Milestone 5

- minimal strategy
- one simple trader
- first working Rust `trade_sim`

### Milestone 6

- broader strategy coverage
- improved logging / metrics
- comparative scenario validation

## Recommendation

The project is feasible and worthwhile if the goal is a usable Rust simulation engine that follows
the established domain semantics while improving code clarity and safety.

The right first step is:

- scaffold the workspace
- define the version-1 event and state model
- implement the sequencer, exchange simulator, and order-state pipeline
- validate behavior against deterministic scenarios and expected state transitions

Only after that should the broader trader and alpha graph be implemented.

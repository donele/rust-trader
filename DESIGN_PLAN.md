# Design Plan

## Goal

Build a Rust trading system that supports both live trading and simulation, with a shared
processing path wherever their responsibilities are the same.

The design emphasizes clear ownership, explicit boundaries, deterministic state transitions, and
an architecture that can support multiple strategies, order sinks, and execution gateways.

## Shared architecture

The common processing flow is:

`Sequencer -> Dispatcher -> StrategyHost -> Strategy -> Trader -> OrderManager -> OrderSink -> ExecutionGateway`

Within that flow, the strategy/services layer is responsible for:

- book updates
- order state updates
- position updates
- risk checks
- metric updates
- dispatch to strategy logic

The sequencer is the ingress and ordering boundary. The dispatcher updates trading-state views and
routes messages by type. The strategy host coordinates order, position, risk, and strategy state.
Trading decisions flow downstream through the trader, order manager, order sink, and execution
gateway.

## Runtime design principles

This section describes the design considerations for the shared runtime. Module names and file
organization may change as the system grows. A one-directional, linear data flow is the preferred
default because it keeps ownership and processing responsibilities easy to follow.

### Shared processing model

External exchange gateways and market-data feeds deliver messages to the ingress boundary. The
shared runtime processes those messages in one direction:

```text
Sequencer
    -> Dispatcher
        -> StrategyHost
            -> Strategy
                -> Trader
                    -> OrderManager
                        -> OrderSink
                            -> ExecutionGateway
```

The arrows describe ownership of the next processing step, not necessarily a synchronous function
call. Market-data updates, execution acknowledgements, and other external messages all enter through
the sequencer and then follow the same downstream processing model. Their external sources may
differ, but the logical ownership and direction remain uniform.

The runtime is best treated as a flat processing pipeline:

1. The sequencer receives one external message.
2. The dispatcher updates shared market state and routes the message by its type.
3. The strategy host updates order, position, and risk state as appropriate.
4. The strategy evaluates the event.
5. If the strategy decides to trade, the downstream order path creates an exchange-bound order.
6. The order sink sends that order to the configured execution gateway.
7. Later exchange responses enter again through the normal ingress path.

Each stage generally does its own work and hands off explicitly. It is preferable to avoid hidden
calls that jump from a downstream stage back into an upstream stage. In particular, the usual
direction is:

- the strategy does not call the dispatcher directly
- the dispatcher does not submit orders to an exchange gateway
- the order sink does not update shared market state
- the execution gateway does not call strategy methods directly
- a response re-enters through the sequencer/ingress path rather than recursively invoking
  the component that originally created the order

These are architectural preferences for keeping the common path understandable, rather than
restrictions on every possible implementation. A deliberate exception can be appropriate when it
improves a clear ownership boundary or models a real system interaction.

This flat structure makes the shared runtime easier to test and keeps processing responsibilities
visible as the system grows. It also gives new engineers a clear mental model of the system, which
reduces onboarding and training time. Explicit boundaries make the code easier to maintain and
debug, while reducing accidental coupling, conflicting ownership, invalid intermediate states,
and changes that introduce subtle bugs or crashes.

### Responsibilities of the shared components

#### Sequencer

The sequencer is an ingress and ordering boundary. It receives market data, control messages,
customer algorithm requests, and execution responses, then passes them into the shared processing
pipeline.

It is not expected to own strategy state, order state, or exchange state. It preserves input
ordering and invokes the next processing stage.

#### Dispatcher

The dispatcher is the message router for the trading runtime. It owns or coordinates the
shared `BookMap` and applies market-data updates before strategy processing.

Its responsibilities are:

- update shared market state
- route each message type to the appropriate typed handler
- deliver `AlgoOrderRequest` to the strategy host
- deliver `OrderUpdate` to order, position, and risk state
- forward control and lifecycle events to the correct downstream handler

The dispatcher is not expected to create exchange-bound orders or submit orders to a gateway. From
the dispatcher downstream, the processing path is intended to be shared between live trading and
simulation, so the dispatcher generally should not need to know which mode is running.

#### Strategy host

The strategy host owns the trading-side services surrounding strategy instances:

- strategy instances
- order manager
- position manager
- risk manager
- strategy-specific service context

The host translates typed routed events into strategy callbacks and keeps state-manager updates
centralized. Strategy callbacks are notifications, not an order-return protocol. Exchange-bound
orders should be created by the downstream trader/order-manager path and sent through an order sink.

#### Strategy

The strategy receives processed market-data, order-state, control, and customer algorithm-request
events. It may update its own state and request trading decisions through the downstream trading
interfaces, but it should not know whether the eventual sink is live or simulated. Time is provided
through the shared clock or event-message model.

#### Trader and order manager

The trader turns strategy decisions into exchange-bound order intent. The order manager owns order
lifecycle state and applies risk checks and state transitions. These components are downstream of
the strategy and upstream of the order sink.

The downstream order path carries trading intent toward the order sink. It is separate from the
incoming message path used by the dispatcher and strategy host.

#### Order sink and execution gateway

The order sink is the polymorphic boundary between the shared order path and the execution
environment:

- a live sink forwards orders to an execution gateway
- another sink implementation can provide a different execution environment

The execution gateway is responsible for exchange-specific protocols and transport. It should not
own strategy state or shared market state.

## Exchange simulator integration

The preceding sections describe live ingress and establish the downstream processing path that can
be shared by live trading and simulation. The simulation mode now adds simulation-specific
coordination around that path: it must replay input messages, model exchange behavior, and deliver
generated responses in a deterministic order.

That additional coordination should remain outside the shared components downstream of the
exchange simulator: the dispatcher, strategy host, strategy, and other shared trading services.
These downstream components should continue to process ordinary messages without knowing whether
they came from a live gateway or a simulation. The simulation sequencer and exchange simulator
provide the mode-specific behavior at the boundary of the shared path.

The exchange simulator is therefore placed between the simulation sequencer and the shared trading
pipeline. It may understand simulation time, fill rules, resting trading orders, and depth simulation
data, while the downstream processing path remains common.

The simulation flow is:

```text
SimulationSequencer
    -> ExchangeSimulator
        -> Dispatcher
            -> StrategyHost
                -> Strategy
                    -> Trader
                        -> OrderManager
                            -> simulated OrderSink
                                -> ExchangeSimulator
```

The simulation sequencer owns the simulation priority queue. A simulation driver or data loader
should only register sources and start the sequencer. It should not inspect message variants, call
the exchange directly, call the dispatcher directly, or handle order updates specially.

### Exchange-first processing

For every message taken from the simulation queue, the exchange simulator gets the first chance to
process it. This preserves the causal rule that the exchange observes market data before the
trading path does.

The exchange and shared downstream path handle messages by their shared type. Their responsibilities
and ownership can be summarized together:

| Message | Origin | Exchange simulator action | Shared downstream action | Primary state owner |
|---|---|---|---|---|
| market data | historical source | Use it for fill/depth simulation and exchange-side state. | Dispatcher updates shared market state and notifies the strategy when the message is delivered. | Exchange simulator and dispatcher/book |
| `AlgoOrderRequest` | customer or external algorithm source | Ignore; this is strategy input. | Dispatcher passes it to the strategy host. | Strategy host and strategy |
| `OrderRequest` | trader/order manager or order sink | Consume if the simulated sink routes it to the exchange simulator. | Do not route through the dispatcher. | Exchange simulator, if used |
| `OrderUpdate` | execution gateway or exchange simulator | Ignore an update generated by the exchange simulator. | Dispatcher updates order, position, and strategy state. | Order and position managers |
| clock/control/reset | runtime or simulation coordinator | Apply only when relevant to exchange state. | Pass through normal dispatcher routing when applicable. | Component-specific |

The exchange simulator must not process an `OrderUpdate` as a newly received order response or
attempt to generate another update from it. The simulation sequencer can send the update through the
same ordinary message path; message-type handling prevents it from re-entering exchange processing
as a new exchange action.

The dispatcher should handle an `OrderUpdate` the same way regardless of whether it came from a live
execution gateway or the exchange simulator. This preserves uniform behavior between live trading
and simulation.

The simulation queue, exchange and strategy timestamps, simulation priorities, exchange-side order and fill state,
and any exchange/trading-path event-target metadata belong to the simulation coordinator or exchange
simulator. They do not belong in the shared dispatcher, strategy host, or strategy APIs.

Shared trading-side components should receive ordinary messages in both live and simulation modes.
The dispatcher should not need to know which mode produced a message.

### Order flow into exchange simulation

The preferred simulation order path is a loop through the global simulation queue:

```text
                         ┌──────────────────────┐
                         │ SimulationSequencer  │ ◄─────────────────────────────┐
                         └──────────┬───────────┘                               │
                                    │                                           │
                                    ▼                                           │
                          ExchangeSimulator                                     │
                                    │                                           │
                                    ▼                                           │
                              Dispatcher                                        │
                                    │                                           │
                                    ▼                                           │
                              StrategyHost                                      │
                                                                  exchange-bound OrderRequest
                                    │                                           │
                                    ▼                                           │
                               Strategy                                         │
                                    │                                           │
                                    ▼                                           │
                                Trader                                          │
                                    │                                           │
                                    ▼                                           │
                              OrderManager                                      │
                                    │                                           │
                                    ▼                                           │
                            simulated OrderSink                                 │
                                    │                                           │
                                    └───────────────────────────────────────────┘
```

The simulated order sink submits an exchange-bound order back to the simulation sequencer. The
sequencer places it in the global queue with its exchange processing timestamp. When that queue entry
becomes current, the appropriate exchange simulator receives the order and applies its own matching,
fill, order-state, and own-order logic.

When the exchange simulator produces an acknowledgement, fill, or other `OrderUpdate`, it sends the
update back to the simulation sequencer. The sequencer places it in the same global queue with its
strategy processing timestamp, so it follows the normal path through the dispatcher and strategy
host.

This central-queue model gives the simulation one deterministic ordering point for market data,
exchange-bound orders, and exchange-generated updates. With multiple exchanges, a simulation-only
queue entry can identify the target exchange simulator. Each exchange instance owns its own order
and fill state, while the sequencer preserves ordering across all exchanges.

An alternative is the hybrid model: the simulated order sink submits directly to an exchange
simulator, which owns a pending-order buffer and exchange-side timing. The exchange still sends
generated updates back through the simulation sequencer. This provides stronger exchange
encapsulation; the exchange advances its internal buffer when it receives the normal clock messages
used by live and simulation runtimes.

Another alternative is for the exchange simulator to own both input and output queues. This further
isolates exchange behavior, but requires an additional coordinator to merge exchange events with
market-data and strategy events. These alternatives remain possible, but the central queue is the
preferred starting point for simple, robust, deterministic behavior.

### Simulation priority queue

The simulation priority queue is owned by `SimulationSequencer`, not by the shared dispatcher or
strategy host. It merges:

- market-data source messages
- customer algorithm-order messages
- exchange-bound order arrivals
- order updates generated by the simulated exchange
- other simulation-generated control or clock messages

The queue orders each entry by the timestamp of its processing component: exchange-bound entries use
`exchange_time`, and strategy-bound entries use `strategy_time`. Within either stream, entries are
ordered by:

1. configured priority
2. monotonic insertion sequence

The insertion sequence is required for deterministic behavior when two events have equal processing
timestamps and priorities.

The queue may use internal simulation-only metadata to distinguish an event generated by the
simulated exchange from a source event, but that metadata must remain inside the simulation
sequencer. It must not appear in
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

Latency should be modeled with exchange and strategy timestamps, not by changing the event's
original exchange timestamp.
At minimum, simulation may eventually need separate latency values for:

- market data from exchange to trading client
- new orders from trading client to exchange
- cancels and modifications from trading client to exchange
- acknowledgements from exchange to trading client
- fills from exchange to trading client

The simulation queue carries both timestamps. `exchange_time` identifies exchange-side processing;
`strategy_time` identifies strategy-side processing:

```rust
struct ScheduledMessage {
    exchange_time: TimeNanos,
    strategy_time: TimeNanos,
    priority: i64,
    sequence: u64,
    message: Message,
}
```

The current phase uses zero latency. Zero latency means that the exchange and trading path process
the event at the same logical time, with the exchange still processing it first. It does not mean
that the exchange is bypassed or that the strategy is called directly from the source.

When a trading-client-facing message is generated by the exchange, the exchange should send it through a
trading-client message sink. The sink is currently allowed to be a no-op while the trading-client processing path is
being built. The important rule is that the exchange should not return an ad hoc separate
trading-client message to application code for manual handling. Once the sink is implemented, it can enqueue the message
with the appropriate strategy time without changing the application loop.

### Exchange and trading-client order books

There are two valid strategies for representing market-data state when latency is modeled:

1. Keep one shared trading-path `BookMap`, owned and updated by the dispatcher and read by both the
   exchange simulator and the strategy. This actual sharing raises two questions: how can the
   exchange and strategy operate at different logical times while preserving event order, and how
   can the exchange perform fill matching without owning the trading-path book? The first question
   is addressed by assigning each queued message exchange and strategy times and ordering it by the
   timestamp of the component that will process it.
   The second is addressed by maintaining additional exchange-side fill/depth state.
2. Maintain two books: an exchange book and a trading-client book. This provides separate exchange
   and strategy-side market views, with each book updated when its corresponding processing event is
   handled.

Unless the simulation later models market-data gaps, drops, reordering, or different book-building
rules for the exchange and strategy, maintaining two books is not needed. Different exchange and
strategy timestamps alone do not justify duplicating the book state.

The one-book model does not attempt to represent two independently timed book views. The exchange
and dispatcher read the same current book representation, while the queue preserves the processing
order of events at their respective logical times. If independent book views are required, the
second model should be used instead.

Both options are viable. The first option is currently selected: the dispatcher owns and updates the
trading-path `BookMap`, the exchange simulator may read it, and the exchange simulator owns its order
list and additional fill/depth state. The second option can be adopted later if separate exchange
and strategy-side market views make the model clearer or more useful.

### Simulation/live parity considerations

The following considerations describe the intended relationship between live and simulation code:

- live and simulation strategies receive the same shared message shapes
- live and simulation dispatchers use the same typed routing behavior
- the sequencer implementation may differ between live and simulation
- the order sink and execution environment may differ between live and simulation
- simulation-specific timestamps and latency scheduling belong to the simulation sequencer and other
  simulation-only components, including the exchange simulator and simulated order sink
- trading order and position state belongs to the shared trading runtime
- exchange order/fill/depth state belongs to the exchange implementation
- an application-level loop is generally clearer when it does not branch on every message type just
  to keep the runtime working

If a new feature requires a simulation-only field in `Message`, first check whether it belongs in a
scheduled queue entry or exchange-simulator state instead. Shared messages should describe domain
events, not the mechanics used to deliver them in one runtime mode.

## Simulation clocks and causality

The simulation uses one deterministic queue with two logical timestamps:

- `exchange_time`: when the exchange processes an event;
- `strategy_time`: when the strategy-side path processes the corresponding event.

The queue orders each entry by the timestamp of its processing component, then by priority and
insertion sequence. This allows the exchange and strategy to operate at different logical times
while preserving causal order. In particular, the exchange processes market data before the
dispatcher updates the shared `BookMap`, and strategy code runs only after that update.

The two-book model is not required merely because the exchange and strategy have different logical
times. The selected model uses one shared market-data book, with separate exchange-side fill and
own-order state. A second book remains an option if independent exchange and strategy market views
become necessary.

Exchange-generated acknowledgements and fills follow the same queue and timestamp rules. The
simulation driver does not handle them specially, and shared downstream components do not need to
know whether a message originated from a live gateway or a simulated exchange.

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
  - simulation CLI entrypoint
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
- deterministic `SimulationSequencer` heap ordered by timestamp, channel priority, and channel insertion order
- half-open replay time-window filtering
- CSV algorithm-order source with symbol lookup, validation, sorting, and `AlgoOrderRequest` emission
- tests for multi-channel merge ordering, tie-breaking, time windows, and CSV input
- streaming Ferris CSV sources for L2 and book-ticker data
- snapshot packet-boundary emission and source-specific parsing errors
- end-to-end feed, sequencer, and `BookMap` processing tests
- full-day ABC and DEF validation from converted Ferris CSV data
- exchange-first, zero-latency market-data handoff to the trading path
- dispatcher-owned trading-path `BookMap`

Still intentionally incomplete in this phase:

- exact packed/wire layouts and shared-memory serialization
- external feed parsing and message deserialization
- symbol registry and symbol metadata
- price/quantity fixed-point-style representation
- complete message families such as control, balance, position, and risk updates
- typed dispatcher routing with a dispatcher-owned `BookMap`
- polymorphic strategy boundary with a boxed `FarPegStrategy`

The current types are therefore a semantic Rust model, not yet a binary-compatible transport model.

## Next steps

Phases 1 and 2 are complete at the primitive level, the historical CSV feed slice is in place, and
the strategy dispatch boundary is implemented. The next work is:

1. Add `Trader`, `OrderManager`, and `OrderSink` integration.
2. Add simulated acknowledgements and basic exchange-side order/depth state.
3. Add the priority queue and non-zero market-data/order/fill latency.
4. Add simulated and live execution-sink interfaces.
5. Implement crossing/fill behavior, cancels, and latency-aware
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
- `SimulationSequencer`

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
- latency-aware order update emission

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

Prefer:

- explicit Rust structs and enums
- traits only where they represent real substitution boundaries
- immutable event values
- mutable hot-path state objects where performance matters

### Separate data model from runtime wiring

The implementation should separate these concerns clearly:

- domain state
- dispatch logic
- runtime object graph wiring
- config-driven assembly

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

Use a top-level shared message enum for events that can travel through the sequencer and shared
runtime:

- `Market(MarketEvent)`
- `AlgoOrderRequest(AlgoOrderRequest)`
- `Simulator(SimEvent)`
- `Control(ControlEvent)`

`OrderRequest` is deliberately not a `Message` variant. It is an exchange-bound value created by
the downstream order path and passed directly to the order sink or exchange simulator. This keeps
it out of the dispatcher and prevents strategy and trading-runtime code from accidentally treating an exchange
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
5. `SimulationSequencer` merges all events
6. `ExchangeSimulator` receives events
7. `StrategyHost` receives market and order events
8. dispatcher routes messages to the strategy host
9. strategy evaluates signals and sends simulated orders
10. exchange simulator schedules simulated acknowledgements / fills / cancels

That is the minimum useful vertical slice.

## Scope cut for version 1

A good version-1 target:

- historical simulation only
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

The first implementation should aim for predictable performance, not maximum cleverness.

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

This is different from optimizing IPC-heavy runtime components. The trading runtime is broader and
more state-heavy than a pure publish/subscribe benchmark.

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
- first working simulation path

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

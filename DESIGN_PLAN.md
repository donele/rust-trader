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

The established runtime flow is:

`SimSequencer -> ExchangeSimulator -> StrategyHost -> TradingEngine`

Within that flow, the strategy/services layer is responsible for:

- book updates
- order state updates
- position updates
- risk checks
- metric updates
- dispatch to strategy logic

The Rust design should preserve that runtime flow and those ownership boundaries, even if the
type names or module layout differ.

## Version 1 scope

The first version focuses on a reliable simulation engine with a clear vertical slice.

Version 1 covers:

- core market and order event modeling
- deterministic replay and sequencing
- exchange simulation
- order, position, and risk state
- strategy host and a simplified trading engine
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
  - trading engine
  - alpha engine
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
- shared order and lifecycle messages: `OrderRequest`, `OrderUpdate`, `AlgoOrderRequest`, `ClockMessage`, `EndTimeMessage`, `PacketBegin`, `PacketEnd`, and `Reset`
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
- full-day SOL and ADA validation from converted Ferris CSV data

Still intentionally incomplete in this phase:

- exact packed/wire layouts and shared-memory serialization
- external feed parsing and message deserialization
- symbol registry and symbol metadata
- price/quantity fixed-point representation
- complete message families such as control, balance, position, and risk updates
- dispatcher and strategy message routing
- polymorphic trader and order-sink interfaces

The current types are therefore a semantic Rust model, not yet a binary-compatible transport model.

## Next steps

Phases 1 and 2 are complete at the primitive level, and the historical CSV feed slice is now in
place. The next work is:

1. Add dispatcher and strategy message routing.
2. Make the strategy boundary polymorphic, starting with `FarPegStrategy`.
3. Add `Trader`, `OrderManager`, and `OrderSink` integration.
4. Add simulated and live execution-sink interfaces.
5. Implement simulated acknowledgements, crossing/fill behavior, cancels, latency, and delayed
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

Implement:

- dispatcher
- `StrategyHost`
- per-symbol market-data service context
- per-message dispatch chain

This layer should preserve the architectural shape:

- pre-process message
- update book / position / risk / order state
- dispatch to strategy / trading engine

Recommended Rust approach:

- use a concrete host struct that owns the state managers
- keep the dispatch interface simple
- avoid deep trait object graphs in version 1

Deliverable:

- a Rust strategy host that receives sequenced events and sees the same logical state transitions

### Phase 7: Trading engine and alpha engine

Implement:

- `TradingEngine`
- `AlphaEngine`
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

Use a top-level shared message enum, for example:

- `Market(MarketEvent)`
- `OrderRequest(OrderRequest)`
- `AlgoOrderRequest(AlgoOrderRequest)`
- `Simulator(SimEvent)`
- `Control(ControlEvent)`

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
8. dispatcher updates state managers
9. trading engine evaluates signals and sends simulated orders
10. exchange simulator schedules simulated acknowledgements / fills / cancels

That is the minimum useful vertical slice.

## Scope cut for version 1

A good version-1 target:

- historical `trade_sim` only
- one replay sequencer
- one exchange simulator
- one order manager / risk / position pipeline
- one strategy host
- one simplified trading engine
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

- minimal trading engine
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

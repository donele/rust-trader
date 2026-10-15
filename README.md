# rust-trade-sim

Rust trading and simulation runtime with shared live/simulation components.

The planned runtime flow is:

`Sequencer -> (Simulator) -> Dispatcher -> StrategyHost -> Strategy -> Trader -> OrderManager -> OrderSink`

`OrderSink` is polymorphic. Simulation uses a simulated sink, while live execution uses an
exchange-specific gateway such as Binance or OKX.

This repo currently contains the first implementation slices for that architecture:

- Cargo workspace with crate boundaries matching the design plan
- shared message, identifier, symbol, and order-book model
- deterministic in-memory and CSV feed sources
- streaming Ferris CSV sources for L2 and book-ticker data
- merged replay sequencer with stable ordering
- sequencer-to-`BookMap` integration with snapshot packet handling
- position and risk-management primitives
- `StrategyHost` with the initial `FarPegStrategy` placeholder
- `trade_sim` binary that exercises the current scaffold

For the design plan, see [DESIGN_PLAN.md](./DESIGN_PLAN.md).

## Version 1 target

Version 1 is a deterministic historical simulator with a clear vertical slice.

It aims to cover:

- core market and order event modeling
- deterministic replay and sequencing
- exchange simulation
- order, position, and risk state
- strategy host and polymorphic strategy layer
- one or two trader implementations
- local configuration loading for offline runs

Version 1 does not aim to cover:

- live shared-memory playback
- exchange gateway integrations
- full strategy and alpha-graph parity
- aggressive low-level optimization

## Repository layout

- `crates/core`
  - shared identifiers, time aliases, and event types
- `crates/feed`
  - replay source and sequencing layer
- `crates/sim-exchange`
  - simulator and simulated execution lifecycle
- `crates/orders`
  - order, position, and risk managers
- `crates/strategy`
  - dispatcher, strategy host, strategies, traders, and order sink integration
- `crates/sim-app`
  - `trade_sim` binary

## Prerequisites

You need a Rust toolchain with `cargo`.

Recommended installation:

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

This scaffold uses only the Rust standard library, so it does not require external crate downloads
 beyond the toolchain itself.

## Build

From the repository root:

```bash
cargo build
```

For an optimized build:

```bash
cargo build --release
```

## Run

Run the scaffold binary from the repository root:

```bash
cargo run -p sim-app --bin trade_sim
```

Expected output is a short scaffold status message including:

- `trade_sim scaffold ready`
- `order_updates_seen=1`

For an optimized run:

```bash
cargo run --release -p sim-app --bin trade_sim
```

To process the converted SOL and ADA Ferris data:

```bash
FERRIS_CSV_ROOT=/mnt/bigdata2/Ferris_csv/OKX/2026/1011 \
  cargo run -p sim-app --bin trade_sim
```

## Current status

This is an early vertical slice, not yet the full simulator or live runtime.

Implemented today:

- workspace and crate boundaries
- shared message and identifier model
- symbol registry and per-symbol order books
- deterministic replay sources and sequencer
- Ferris CSV L2 and book-ticker ingestion
- end-to-end feed, sequencer, and `BookMap` processing
- position and risk-management primitives
- simulated order acknowledgement scaffold
- `StrategyHost` and initial `FarPegStrategy` type

Still to implement for version 1:

- dispatcher and strategy message routing
- `Trader`, `OrderSink`, and execution-gateway interfaces
- complete strategy behavior, beginning with far-peg orders
- exchange fill, cancel, latency, and delayed-update semantics
- configuration-driven live and simulation wiring

## Next development steps

The next implementation phase is:

1. add dispatcher and strategy message routing
2. add the polymorphic `Trader` interface
3. add `OrderManager` to `OrderSink` integration
4. add simulated and live execution-sink interfaces
5. implement simulated acknowledgements, fills, cancels, latency, and delayed updates

# Chat2DB Rust

Private implementation of the Chat2DB Community hybrid runtime.

## Current state

The repository currently provides the first three buildable stages:

- canonical Rust API contracts;
- a transport-neutral Rust application service root;
- an Axum health API bound to loopback by default;
- a Rust CLI status command;
- a React runtime-status shell;
- one Protobuf 1.0 lifecycle and JDBC contract generated in Rust and Java;
- a supervised Rust client with handshake, ping, bounded stderr capture,
  request correlation, crash reporting, and forced/clean shutdown;
- an external-driver Java 17 compatibility engine whose stdout is
  protocol-only;
- Rust driver, session, transaction, update, and credit-streaming query APIs;
- typed row batches, cancellation, deadlines, bounded results, and explicit
  unknown-outcome handling; and
- real Rust-to-Java H2 integration tests with H2 outside the engine JAR.

SQLite storage, Tauri, AI, MCP, signed driver packs, and the existing Chat2DB
plugin/ANTLR estate are tracked as explicit staged work in
[`docs/stages.md`](docs/stages.md). The bootstrap Web and core composition do
not yet start the Java supervisor, so runtime health continues to report the
database engine as disabled. This avoids presenting an integration-tested
bridge as a product-wired database service.

## Architecture

The final runtime is intentionally hybrid:

```text
React / TypeScript
  -> Tauri IPC or Axum HTTP
  -> Rust product runtime
  -> framed Protobuf IPC
  -> private Java compatibility engine
  -> Chat2DB plugins + JDBC + Java ANTLR
  -> database
```

See [`docs/architecture.md`](docs/architecture.md) for ownership and protocol
decisions and [`docs/protocol.md`](docs/protocol.md) for the implemented 1.0
process contract.

## Build

Prerequisites:

- Rust 1.88 or newer;
- Java 17; the checked-in Maven Wrapper downloads Maven 3.9.12;
- Node.js 22.12 or newer within the Node 22 release line, and npm 10.9.7.

Run all current verification gates, including real Rust-to-Java process tests:

```bash
make verify
```

Java verification downloads H2 `2.3.232` into
`java/compat-runtime/target/test-drivers/` as an external Stage 3 test fixture.
H2 is not a runtime dependency of the compatibility engine, and the packaged
JAR integration test rejects any build that embeds `org/h2/Driver.class`.
The real H2 gate covers the implemented Stage 3 JDBC vertical slice; H2 is a
test fixture rather than a bundled product driver.

Run the Web API:

```bash
cargo run -p chat2db-web
```

Run the frontend in another terminal:

```bash
cd apps/frontend
npm run dev
```

The Web API listens on `127.0.0.1:4200` by default. The Vite development server
listens on `127.0.0.1:4210` and proxies `/api` to the Rust runtime.

## Repository layout

```text
apps/
  chat2db-cli/       Rust command-line adapter
  chat2db-web/       Axum Web adapter
  frontend/          shared React application
crates/
  chat2db-contract/  canonical DTOs and errors
  chat2db-core/      transport-neutral product services
  chat2db-engine-protocol/ generated internal wire types and frame codec
  chat2db-java-bridge/ supervised Java process client
proto/               canonical Rust-Java process schema
java/
  compat-runtime/    private Java compatibility process
docs/                architecture and staged delivery contract
```

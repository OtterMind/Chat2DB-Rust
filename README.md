# Chat2DB Rust

Private implementation of the Chat2DB Community hybrid runtime.

## Current state

The repository currently provides the first five buildable stages:

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
- real Rust-to-Java H2 integration tests with H2 outside the engine JAR;
- process-locked SQLite storage with verified WAL, foreign keys, full
  synchronous writes, transactional migrations, and startup integrity checks;
- revisioned datasource metadata whose complete connection descriptor lives
  behind an injected, readiness-checked secret-vault boundary; and
- disk-backed Protobuf result frames with SHA-256 indexes, row/byte-bounded
  paging, a physical-byte quota, expiry, writer cleanup, and crash recovery;
- an AES-256-GCM encrypted file vault rooted in either an OS-keyring master key
  or an explicit headless master key;
- one production `RuntimeHost` that opens the vault and storage, supervises the
  Java engine, and shuts down active work deterministically;
- secret-safe datasource CRUD, asynchronous query operations, bounded replay,
  explicit cancellation, and retained-result paging through Axum JSON/SSE and
  Tauri 2 commands/channels;
- a checked-in OpenAPI contract with generated TypeScript types and drift
  verification; and
- one shared React SQL workbench with HTTP and Tauri backend adapters.

AI, MCP, signed driver packs, and the existing Chat2DB plugin/ANTLR estate are
tracked as explicit staged work in [`docs/stages.md`](docs/stages.md). Web and
desktop compose the complete Stage 5 runtime; the current CLI remains a
status-only adapter. Driver loading is proven through the internal H2 product
fixture, while user-facing driver-pack provisioning remains Stage 7.

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
The H2 gates cover both the Stage 3 bridge and the Stage 5 product path from a
vault-backed datasource through Java streaming into retained-result paging and
cancellation. H2 is a test fixture rather than a bundled product driver.

Generate or verify the external contracts:

```bash
make generate-contracts
make check-contracts
```

Build the Java engine and shared frontend, then run the Web product host:

```bash
make java frontend
CHAT2DB_JAVA_ENGINE_JAR="$PWD/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
CHAT2DB_VAULT_MASTER_KEY="$(openssl rand -base64 32)" \
cargo run -p chat2db-web
```

Run the frontend in another terminal:

```bash
cd apps/frontend
npm run dev
```

The Web API listens on `127.0.0.1:4200` by default. The Vite development server
listens on `127.0.0.1:4210` and proxies `/api` to the Rust runtime.
`CHAT2DB_DATA_DIR` selects a profile directory. Omitting
`CHAT2DB_VAULT_MASTER_KEY` selects the OS credential store. Any non-loopback
`CHAT2DB_BIND` also requires `CHAT2DB_ACCESS_TOKEN` with at least 32 bytes.

## Repository layout

```text
apps/
  chat2db-cli/       Rust command-line adapter
  chat2db-desktop/   Tauri 2 desktop adapter
  chat2db-web/       Axum Web adapter
  frontend/          shared React application
contracts/openapi/   generated external HTTP contract
crates/
  chat2db-contract/  canonical DTOs and errors
  chat2db-core/      transport-neutral product services
  chat2db-engine-protocol/ generated internal wire types and frame codec
  chat2db-java-bridge/ supervised Java process client
  chat2db-storage/   SQLite state, secret references, and retained results
proto/               canonical Rust-Java process schema
java/
  compat-runtime/    private Java compatibility process
docs/                architecture and staged delivery contract
scripts/             contract generation and drift checks
```

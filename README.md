# Chat2DB Rust

Private implementation of the Chat2DB Community hybrid runtime.

## Current state

The repository currently provides the buildable bootstrap surface:

- canonical Rust API contracts;
- a transport-neutral Rust application service root;
- an Axum health API bound to loopback by default;
- a Rust CLI status command;
- a React runtime-status shell;
- a Java 17 compatibility-process identity and test baseline.

Database IPC, JDBC sessions, SQLite storage, Tauri, AI, MCP, driver packs, and
the existing Chat2DB plugin/ANTLR estate are tracked as explicit staged work in
[`docs/stages.md`](docs/stages.md). Disabled components are reported as disabled;
the bootstrap build does not pretend that those capabilities are implemented.

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
decisions.

## Build

Prerequisites:

- Rust 1.88 or newer;
- Java 17; the checked-in Maven Wrapper downloads Maven 3.9.12;
- Node.js 22.12 or newer within the Node 22 release line, and npm 10.9.7.

Run all current verification gates:

```bash
make verify
```

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
java/
  compat-runtime/    private Java compatibility process
docs/                architecture and staged delivery contract
```

# Chat2DB Rust

Source-available implementation of the Chat2DB Community hybrid runtime.

Chat2DB Rust owns the product runtime in Rust, uses native Rust paths for the
MySQL, PostgreSQL, SQL Server, and Oracle browser and Console data planes, and
uses a Rust-owned DM adapter over the generic JDBC bridge. The broader public
Chat2DB Community database compatibility layer remains behind a supervised Java
process. The repository is under active development and is not yet a stable
end-user release.

## Clone

The Community compatibility source is a public Git submodule pinned to an
exact commit. Clone it together with the main repository:

```bash
git clone --recurse-submodules https://github.com/OtterMind/Chat2DB-Rust.git
cd Chat2DB-Rust
```

For an existing checkout, initialize the same pinned source with:

```bash
git submodule update --init --recursive
```

## Current state

The repository has completed Stages 1 through 6, the first thirteen
independently buildable Stage 7 slices, and the complete MySQL workbench
surface reached by the pinned Community frontend:

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
- one production `RuntimeHost` that opens the vault, storage, and verified
  driver catalog without starting Java, then single-flights first-use startup,
  leases each generation to active work, reaps it after three idle minutes,
  reloads drivers on the next use, and shuts down deterministically;
- secret-safe datasource CRUD, asynchronous query operations, bounded replay,
  explicit cancellation, and retained-result paging through Axum JSON/SSE and
  Tauri 2 commands/channels;
- a checked-in OpenAPI contract with generated TypeScript types and drift
  verification; and
- the original Community Umi/React layout, components, and styles, served by
  Axum over historical HTTP routes on Web and bridged from `window.javaQuery`
  to Tauri IPC on desktop without a replacement UI or style fork; a locked,
  reviewable host-adapter patch supplies CSP-safe callbacks plus Web/Desktop
  file upload and download transport compatibility;
- a provider-neutral bounded agent loop with direct OpenAI, Anthropic, and
  Gemini adapters, durable sessions/messages/runs/permissions, and atomic
  context compaction;
- shared `query_database`, `inspect_query_result`, and
  `execute_database_write` tools with read-only enforcement, per-call write
  approval, bounded result previews, run-bound handles, and conservative
  unknown-outcome handling; and
- durable Agent run start, snapshot, cancellation, permission decision, and
  replay/live streaming through Axum SSE, Tauri channels, and matching
  frontend HTTP/Tauri observers;
- an authenticated owner-only local attachment started by both product hosts,
  plus a JSON CLI for datasource discovery, forced-read-only query lifecycle,
  cancellation, retained-result paging, and explicitly confirmed MySQL writes;
  and
- an `rmcp` 2.2 stdio server with six bounded datasource/query tools, including
  a MySQL write tool backed by that same running `Application`; writes require
  protocol-level Form elicitation from the trusted client, while model-visible
  tool arguments expose neither `confirm` nor an approval token;
- strict local JDBC driver-pack discovery, hash verification, immutable
  Core/Axum/Tauri inventory, and repeatable per-generation preload; and
- a fixed Community 5.3.0 compatibility classpath that discovers real
  `IPlugin` implementations and exposes H2 plugin catalog, schema, object,
  view, foreign-key, primary-key, function, procedure, parameter, and trigger
  metadata, `CREATE SCHEMA` builder, and retained ANTLR parser operations over
  Protobuf, with every one of its 149 JARs bound to the source commit by a
  checked-in filename, byte-length, and SHA-256 lock; and
- product-owned Community DTOs and Core services exposed consistently through
  Axum and Tauri, with exact locked-classpath startup and forced-read-only
  metadata sessions; and
- an original-frontend compatibility layer for native MySQL connection testing,
  datasource CRUD/tree, database/schema/table discovery, compact table lists,
  columns, indexes, keys, views, functions, procedures, triggers, and synchronous
  table preview over the historical `{success,data,errorCode,errorMessage}`
  envelope, including the original DDL-list `total` field; and
- durable SQLite-backed Community Console create/get/list/update/delete,
  including SQL text, datasource/database/schema binding, saved status, and
  open-tab state across process restarts; and
- Community Console execution through upstream `mysql_async 0.37.0` without
  starting Java: unparameterized reads, DDL/DML, semicolon and `DELIMITER`
  scripts, multiple result sets, explicit transactions, error-continue policy,
  preserved-single dispatch, `EXPLAIN`, normal/all-row paging, datasource
  read-only enforcement, cancellation, a shared 64 MiB result budget, bounded
  large-cell tokens/downloads, and durable per-statement history; Web uses the
  historical synchronous result shape while desktop emits the original JCEF
  statement/result/row/update-count lifecycle;
- a shared Web/Tauri legacy dispatcher: Axum maps the original `/api` routes,
  while desktop preserves the original JCEF correlation envelope through one
  `legacy_request` Tauri command; and
- forced-read-only table preview that ignores caller SQL, generates a bounded
  SELECT through the selected Community plugin, and pages retained results;
- complete native MySQL datasource lifecycle, SSH tunneling, portability,
  metadata, editable DML and DDL, views and routines, transfer tasks, account
  administration, schema diff, pins, ER layout, workspace persistence, and
  SQLite-backed Dashboard/Chart CRUD with native read-only chart refresh;
- native PostgreSQL through `tokio-postgres 0.7.18`, with connection and SSH,
  retained queries and typed parameters, cancellation and limits, Console,
  relational and programmability metadata, table DDL and preview, ER metadata,
  and native schema/namespace/DML builders;
- native SQL Server through `tiberius 0.12.3`, with the same relational
  workbench slice, TDS-aware result handling, direct-batch Console semantics,
  conservative write cancellation, and native schema/namespace/DML builders;
- native Oracle through the pure-Rust `oracle-rs 0.1.7` protocol client, with
  connection and SSH, retained queries, Console, metadata, DDL, preview, ER,
  and native schema/namespace/DML builders without OCI, ODPI-C, JDBC, or Java;
  unsupported lossy Oracle result types fail closed instead of fabricating
  values; and
- the pinned Community AI workspace routes plus confirmed Agent, CLI, and MCP
  writes, with explicit approval, read-only enforcement, single-statement
  validation, and conservative unknown-outcome handling.

Runtime-tested: yes. The Stage 7M product vertical passed against a real local
MySQL 8.4 server on 2026-07-27, including plugin-built qualified table SQL,
forced-read-only execution, and retained paging of the selected table's rows.
Commit `928e62c5d775d0e81d95db7fee186db756834a72` additionally passed the
complete local repository gate and a live original-frontend legacy HTTP
vertical covering connection, datasource persistence, database/table listing,
and three-row table preview. On 2026-07-28 commits `36ecac6`, `78e92d6`, and
`c51fdff` passed formatting, strict workspace Clippy, all 509 Rust tests, 49
frontend tests, and the Community production build. A live MySQL 8.4 run then
created a Console, returned real table rows, returned a renderable SQL error,
saved edited SQL, closed it, restarted the Rust host, reopened it, and executed
the restored SQL successfully. On 2026-07-29 commits `81301c3`, `4199862`, and
`6c74421` passed 144 Core unit tests, strict Core Clippy, and a real MySQL 8.4
vertical covering native connection, database/schema/table discovery, preview,
typed Console SELECT, row truncation, active-query cancellation, retained
paging, and proof after every operation that Java remained dormant.
The complete repository `make verify` gate and the explicit real-MySQL direct
and SSH integration targets also passed after the final compatibility fix.
The metadata parity increment adds a real MySQL fixture with a foreign key,
composite index, view, function, procedure, and trigger; its Core product test,
Axum queries, and desktop dispatch contracts pass while Java remains dormant.
The legacy boundary matches paged table searches against names or comments,
ignores `searchKey` on Community's complete-list endpoints, validates metadata
`pageSize` in `1..=100000`, returns binding failures in the HTTP 200 JSON
envelope, and preserves `defaultValue: null` separately from an empty-string
default. The native Console integration additionally passes against MySQL 8.4
for DDL/DML, `DELIMITER` procedures, multi-results, transactions, error
continuation, cancellation, a 6 MiB `LONGTEXT`, `single`, `EXPLAIN`,
`pageSizeAll`, and datasource read-only protection while Java remains dormant.
The Dashboard/Chart integration additionally passed against MySQL 8.4 with a
selected database, a 200-row response cap, SELECT CTE support, response-only
refreshed metadata with Community primary-key/auto-increment/nullability/default
and comment headers, rejected writes/multi-statements/locking reads/server-file
output, `CHART` operation history, fixture cleanup, and Java dormant.
The complete `rtk make verify` gate then passed with the Dashboard/Chart
increment included.

On 2026-08-07 the added native PostgreSQL, SQL Server, and Oracle paths passed
real product verticals against PostgreSQL 17, Azure SQL Edge's SQL Server TDS
endpoint, and Oracle Free 23 respectively. The verticals cover connection,
metadata, retained query, Console, preview, DDL/dialect behavior, read-only
enforcement, cancellation, bounded values, cleanup, and continuous proof that
Java remained dormant. Core passed `357` all-target tests with `9` ignored plus
strict Clippy under both the default toolchain and the minimum Rust `1.88.0`.
For Oracle, `oracle-rs 0.1.7` has a fixed 100-row initial prefetch whose legacy
decoder cannot safely expose `BINARY_FLOAT`, `BINARY_DOUBLE`, `ROWID`, or
`UROWID`; those result columns return `oracle_result_type_not_supported` when
the driver exposes their type, and the project does not fork or vendor the
upstream crate.

Stage 6 and the Stage 7A-7M foundations are complete. Web and desktop own the
product runtime and publish its owner-only local endpoint; CLI and MCP attach to
that host and never contact Java directly. The pinned Community frontend's
complete MySQL workbench surface is mapped through the shared Axum/Tauri legacy
dispatcher. Native MySQL connections, metadata, Console, mutations, transfer,
class generation, accounts, schema diff, chart refresh, and workspace operations
remain in Rust and do not acquire a Java lease. PostgreSQL, SQL Server, and
Oracle connection, relational workbench, and dialect-builder operations also
remain in Rust when their explicit native driver ids are persisted. Existing
managed JDBC datasources continue through Java; registering a native driver
never silently changes their execution engine. Dashboard and chart documents
remain in SQLite. Community parser, formatter, completion, and exact plugin
compatibility operations remain Java-backed and start the supervised process
only on demand.

The Console compatibility path uses SQLite migrations 3 and 4 for saved
Consoles and durable execution history. Historical `/api/operation/saved/*`,
`/api/operation/log/*`, `/api/rdb/dml/execute`, `/execute_ddl`, and large-cell
routes share the same native Core execution. Desktop `sql-execute` and
`sql-cancel` keep active cancellation handles and emit row payloads exactly once
through Tauri. Native typed SELECT bind parameters use the MySQL prepared
protocol; the pinned Community write request has no bind-parameter field.

The Stage 5 and Stage 7G through Stage 7M custom React workbench was an
intermediate implementation and is no longer the product frontend. Commit
`928e62c5d775d0e81d95db7fee186db756834a72` deletes that replacement UI and
its styles. Current builds export the original Community frontend from the
pinned submodule plus the locked host-transport compatibility patch. Every
historical API used by its MySQL workbench is mapped;
cloud account, login, payment, subscription, invitation, notification, and
Enterprise-only features are outside this database milestone. Signing,
downloading, updating, rollback, and full compatibility for other database
dialects remain follow-on work.

## Architecture

The final runtime is intentionally hybrid:

```text
React / TypeScript              CLI / MCP client
  -> Tauri IPC or Axum HTTP       -> JSON CLI or MCP stdio
                                  -> owner-only local attachment
                    \            /
                     -> Rust product runtime
  -> framed Protobuf IPC
  -> private Java compatibility engine
  -> Chat2DB plugins + JDBC + Java ANTLR
  -> database
```

See [`docs/architecture.md`](docs/architecture.md) for ownership,
[`docs/protocol.md`](docs/protocol.md) for the implemented 1.0 process contract,
and [`docs/driver-packs.md`](docs/driver-packs.md) for the local manifest and
startup contract. [`docs/mysql-community-parity.md`](docs/mysql-community-parity.md)
is the source-locked acceptance contract for matching the original Community
frontend's complete MySQL feature surface.

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
The H2 gates cover the Stage 3 JDBC bridge, the Stage 5 product path from a
vault-backed datasource through retained-result paging and cancellation, and
the Stage 7B Community path through real `IPlugin`, `IDbMetaData`,
`ISqlBuilder`, and ANTLR parser implementations. The Stage 7C-7M product gate
repeats those calls through encrypted datasource storage and Core services,
including forced-read-only schema, object, relation, and programmability
metadata session cleanup plus datasource-free parsing, validation, formatting,
typed DML generation, namespace SQL generation, plus datasource-bound table and
column completion and table preview. H2 remains an external test driver rather
than a runtime dependency of either Java classpath.
The Stage 7J completion workbench also passes Playwright visual acceptance at
desktop `1440x1000` and mobile `390x844` viewports, with no overlapping or
out-of-bounds content, horizontal page scrolling, or text overflow.

Build and run only the fixed Community H2 compatibility gate with:

```bash
make community-h2-integration
make community-product-h2-integration
```

Prepare the pinned MySQL Connector/J pack and run the complete real MySQL
product vertical against a local server:

```bash
MYSQL_TEST_USER=root \
MYSQL_TEST_PASSWORD='<local password>' \
make community-product-mysql-integration
```

Run the native MySQL product path alone, without building Java or Connector/J:

```bash
MYSQL_TEST_USER=root \
MYSQL_TEST_PASSWORD='<local password>' \
make native-mysql-integration
```

`MYSQL_TEST_HOST` and `MYSQL_TEST_PORT` default to `127.0.0.1:3306`. The test
creates a uniquely named database, verifies driver loading, datasource CRUD,
metadata, namespace DDL, typed DML, parsing, validation, formatting, completion,
plugin-built table-preview SQL, forced-read-only query execution, and
retained-result paging, then drops the database even when verification fails.
Connector/J is downloaded from Maven Central only after its pinned byte length
and SHA-256 are verified; it remains an external driver pack and is never
embedded in the Java engine.

Prepare and verify the pinned DM JDBC driver pack without requiring a running
DM server:

```bash
make dm-driver-pack-integration
```

The gate starts the project-owned Java 17 JDBC engine and loads only
`dm.jdbc.driver.DmDriver` from the managed pack. DM capability routing,
metadata SQL, validation, result mapping, and bounded table-preview SQL belong
to the Rust `DmDriver` registered in the unified native-driver SPI. The Java
engine is only the generic JDBC transport for the official vendor JAR; it does
not load or invoke the Chat2DB Community DM or Oracle plugins.

A separate product gate proves that the Rust-owned DM SPI works without any
Community classpath or Community database plugin configured:

```bash
make dm-product-integration
```

A live product path is also exercised by this target when `DM_TEST_HOST`, `DM_TEST_PORT`,
`DM_TEST_USER`, and `DM_TEST_PASSWORD` are all set; use `DM_TEST_REQUIRED=1` to
make their absence an error. `DM_TEST_JDBC_URL` can override the default
`jdbc:dm://<host>:<port>` URL. The live test covers connection, database,
schema, table, and column discovery, bounded preview/query execution, and
fixture cleanup. Without an endpoint, the gate still verifies the Driver Pack
and Rust SPI identity without loading a Community classpath.

The public macOS package does not bundle the proprietary DM JDBC JAR because
the JAR contains no verifiable redistribution grant. For local testing, prepare
the pinned pack explicitly and point Web or Desktop at it with
`CHAT2DB_DRIVER_PACK_DIR`.

Those targets require a clean submodule at the fixed commit, build through the
checked-in Maven Wrapper and a repository-local Maven cache, derive archive
timestamps from the commit, exclude the H2 JDBC driver, and deterministically
remove dependency-manifest `Class-Path` entries before rejecting any JAR set
that differs from `third_party/community-h2-classpath.lock`. Run
`make community-h2-reproducibility` to compare every artifact byte across two
consecutive clean builds.

Generate or verify the external contracts:

```bash
make generate-contracts
make check-contracts
```

Build the Java engine, fixed Community classpath, verified MySQL driver pack,
and shared frontend, then run the Web product host with the Stage 7C-7M services
enabled:

```bash
make java community-h2-classpath mysql-driver-pack frontend
CHAT2DB_JAVA_ENGINE_JAR="$PWD/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
CHAT2DB_COMMUNITY_CLASSPATH_DIR="$PWD/target/community-h2-classpath" \
CHAT2DB_DRIVER_PACK_DIR="$PWD/target/driver-packs" \
CHAT2DB_VAULT_MASTER_KEY="$(openssl rand -base64 32)" \
cargo run -p chat2db-web
```

Run the frontend in another terminal:

```bash
cd apps/frontend
npm run dev
```

The Web API listens on `127.0.0.1:4200` by default. The pinned Community Umi
development server listens on `127.0.0.1:4210` and proxies `/api` to the Rust
runtime. Frontend commands export the exact Git tree recorded in
`scripts/community-frontend.lock.json` into `target/`; they never install into
or modify the `third_party/chat2db-community` submodule worktree.
`CHAT2DB_DATA_DIR` selects a profile directory. Omitting
`CHAT2DB_VAULT_MASTER_KEY` selects the OS credential store. Any non-loopback
`CHAT2DB_BIND` also requires `CHAT2DB_ACCESS_TOKEN` with at least 32 bytes.

The running Web or desktop host also publishes the owner-only local endpoint
used by CLI and MCP. Point either adapter at the same profile explicitly when
the default data directory is not used:

```bash
cargo run -p chat2db-cli -- --data-dir /path/to/profile datasources
cargo run -p chat2db-mcp -- --data-dir /path/to/profile
```

MCP clients launch `chat2db-mcp` as a stdio server. Its stdout is reserved for
JSON-RPC; diagnostics use stderr. `CHAT2DB_MCP_LOG` can raise logging for the
Chat2DB target only, while dependency logs remain capped at `WARN` so SQL and
result payloads are not emitted by `rmcp` debug tracing.

## Repository layout

```text
apps/
  chat2db-cli/       Rust command-line adapter
  chat2db-desktop/   Tauri 2 desktop adapter
  chat2db-mcp/       bounded stdio MCP adapter
  chat2db-web/       Axum Web adapter
  frontend/          build harness and transport tests for the pinned Community UI
contracts/openapi/   generated external HTTP contract
crates/
  chat2db-agent/     provider adapters and bounded agent/tool runtime
  chat2db-contract/  canonical DTOs and errors
  chat2db-core/      transport-neutral product services
  chat2db-engine-protocol/ generated internal wire types and frame codec
  chat2db-java-bridge/ supervised Java process client
  chat2db-local/     owner-only CLI/MCP attachment protocol
  chat2db-local-ipc-windows/ Windows named-pipe and ACL implementation
  chat2db-storage/   SQLite state, secret references, and retained results
proto/               canonical Rust-Java process schema
java/
  compat-runtime/    private Java compatibility process
docs/                architecture and staged delivery contract
scripts/             contract generation and drift checks
```

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Contributions are accepted under the
repository license.

## Security

Do not report vulnerabilities in public issues. Follow
[`SECURITY.md`](SECURITY.md) to submit a private report.

## License

Chat2DB Rust is source-available under `LicenseRef-Chat2DB`. See
[`LICENSE`](LICENSE) for the complete terms. The pinned Chat2DB Community
submodule and other third-party components remain subject to their own license
terms and notices.

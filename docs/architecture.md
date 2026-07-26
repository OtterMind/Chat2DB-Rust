# Architecture

## Status

The repository has completed the buildable baseline, versioned Rust-to-Java
process protocol, JDBC vertical slice, local storage foundation, Web/desktop
product transports, and Stage 6 Agent/CLI/MCP delivery. Stage 6 includes direct
provider adapters, the bounded durable Agent runtime, SQL read/write tools,
explicit write permissions, Web/Tauri run transports, an owner-only local
attachment, a read-query CLI, and an `rmcp` stdio server. The implemented Java
bridge loads external driver JARs, owns sessions and local transactions,
executes updates, and streams typed query batches with credits, cancellation,
deadlines, hard limits, and explicit unknown outcomes. The complete
storage-to-Java-to-retained-result path is cross-language tested against H2
without embedding H2 in the compatibility-engine JAR.

The Web and Tauri hosts open the production vault, SQLite storage, and Java
supervisor before exposing a shared `Application`. Axum serves JSON, SSE, and
the React SPA; Tauri exposes commands and per-subscription channels without a
localhost product server. Both hosts also publish an owner-only local endpoint
for the CLI and MCP process. That same `Application` owns query and Agent run
execution, replay, cancellation, and write-permission decisions. Strict local
managed driver packs and immutable inventory are implemented. A fixed Community
5.3.0 submodule now supplies a real H2 compatibility slice for plugin discovery,
schema metadata, dialect SQL building, and retained ANTLR parsing. Product Core,
Axum, Tauri, and both frontend backend adapters expose those four operations
when the exact locked classpath is configured. Signing, distribution, end-user
Community workflows, the remaining dialect estate, and packaging remain target
components. CLI and MCP attach to a running host rather than composing a second
product runtime.

## Ownership

| Area | Owner | Boundary |
| --- | --- | --- |
| Shared UI | React and TypeScript | One transport-neutral backend client |
| Desktop | Rust / Tauri 2 | Tauri commands and events; no product localhost HTTP |
| Web | Rust / Axum | JSON HTTP and SSE |
| Product services | Rust | Workspace, state, policy, tasks, dashboards, and orchestration |
| Durable state | Rust | SQLite, retained-result files, and a mandatory injected secret-vault contract |
| AI agent | Rust | Provider adapters, tool loop, limits, compaction, and cancellation |
| MCP and CLI | Rust | Adapters around the same product services and policy |
| Database compatibility | Java 17 | Existing SPI/plugins, JDBC, metadata, builders, and execution |
| SQL parsing | Java 17 | Existing Java ANTLR grammars, parser behavior, and completion |
| Rust-to-Java IPC | Shared Protobuf contract | Length-prefixed frames over private stdin/stdout |

## Process topology

```text
Desktop                         Web / Docker
React in system WebView         React in browser
  -> Tauri IPC                    -> HTTP / SSE
  -> Rust host                    -> Axum Rust host
          \                       /
           -> Rust application services
              <- owner-only local attachment <- CLI
              <- owner-only local attachment <- rmcp stdio server <- MCP client
              -> SQLite and result store
              -> AI agent runtime
              -> Java process supervisor
                 -> Protobuf stdin/stdout
                 -> Java database compatibility engine
                    -> plugin registry
                    -> JDBC sessions and transactions
                    -> metadata and SQL operations
                    -> Java ANTLR parsers
                    -> vendor driver
```

Only Rust exposes product transports. Java has no listening port. JDBC
connections, statements, result sets, parser trees, and Java exceptions remain
inside the Java process.

## Contract rules

- Rust DTOs are the source of truth for frontend request, response, error, and
  event contracts.
- Desktop and Web adapters call the same application services and contain no
  business rules.
- The Java protocol is versioned and capability-negotiated.
- Every cross-process request carries request, optional session, deadline,
  cancellation, and trace identity. Responses add stream sequence and terminal
  state while echoing request and trace identity.
- Typed row batches use explicit backpressure; stdout is protocol-only and
  stderr is diagnostic-only.
- Read-only metadata/parser work may retry after a Java restart. Transactions,
  DML, and unknown-outcome operations are never replayed automatically.

The implemented lifecycle and JDBC subset is documented in
[`protocol.md`](protocol.md). Capabilities are advertised only after their
cross-language acceptance gates pass.

## Database boundary

Java remains the primary database implementation at the first complete release.
Rust does not reimplement vendor wire protocols and does not introduce parallel
native-driver behavior before the Java-backed conformance baseline passes.

The JDBC baseline implements:

- verified external JAR snapshots and per-driver classloader isolation;
- JDBC session and local-transaction ownership;
- prepared query and update execution with typed parameters;
- typed row batches with row, byte, frame, and scalar limits;
- credit flow control, cancellation, deadlines, and conservative outcomes.

Stage 7B additionally implements:

- a Git submodule fixed at Community commit
  `f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7`;
- a reproducible 148-JAR H2 compatibility classpath whose filenames, byte
  lengths, and SHA-256 digests are bound to that commit by the checked-in
  `third_party/community-h2-classpath.lock`;
- deterministic build-time removal of dependency-manifest `Class-Path` entries,
  with affected JARs rebuilt as sorted, commit-timestamped `STORED` archives;
- a separately supplied Community classpath loaded by a `URLClassLoader` whose
  parent is only the Java platform classloader;
- `ServiceLoader<IPlugin>` discovery projected into stable Protobuf DTOs;
- H2 schema metadata through `IDbMetaData.schemas`, `CREATE SCHEMA` through the
  metadata-owned `ISqlBuilder`, and H2 syntax through its retained MySQL ANTLR
  parser; and
- a cross-language test that executes those operations against an H2 JDBC
  session loaded through the existing external-driver path.

Community plugin objects, JDBC objects, parser objects, and exceptions remain
inside Java. The Community classpath and each JDBC driver classloader are
separate; JDBC driver JARs are not added to the Community classpath. Only
bounded, process-neutral DTOs cross Protobuf.

Stage 7C composes that boundary into the product runtime. The Web and desktop
bootstrap paths accept `CHAT2DB_COMMUNITY_CLASSPATH_DIR`, but the source commit
and 148 filenames, byte lengths, and SHA-256 digests come only from the lock
embedded in `chat2db-core`; environment configuration cannot replace them.
Core exposes catalog, schema metadata, `CREATE SCHEMA`, and parser services.
Schema metadata resolves the encrypted datasource connection and always opens
a forced-read-only JDBC session. Axum publishes four JSON routes with generated
OpenAPI/TypeScript contracts; Tauri and the shared frontend backend client
publish matching calls. The runtime health component distinguishes ready,
disabled, and unavailable compatibility states from both fixed-classpath
configuration and the negotiated engine state. Core runs schema metadata work
in a bounded independent task so transport cancellation cannot skip the session
close path.

The full database plugin inventory, metadata tree, builders, type conversion,
non-relational operations, script execution, formatting, validation,
completion, end-user Community UI workflows, and per-dialect conformance are not
implemented yet. The current product slice proves H2 only.

Spring Boot, Spring Web, Spring AI, MCP, JCEF, product storage, and updater logic
do not belong in the final Java engine.

## Local storage boundary

Stage 4 implements one exclusively process-owned data directory with owner-only
permissions. Every SQLite connection verifies WAL mode, foreign keys, and
`synchronous=FULL`; startup applies an explicit transactional schema migration,
runs integrity and foreign-key checks, and performs idempotent recovery before a
`Storage` handle is exposed.

SQLite stores datasource id, display name, driver id, opaque `SecretRef`,
revision, and timestamps. The complete connection descriptor, including JDBC
URL and properties, is one `SecretValue` owned by the injected vault and never
enters SQLite. Vault creation is create-only. A durable cleanup queue plus a
single secret mutation gate covers staging, datasource CAS, commit
reconciliation, rotation, and deferred deletion. Storage startup requires the
vault readiness probe to succeed.

Interactive hosts root the encrypted file vault in one random 32-byte master
key stored by the OS credential service. Each immutable secret record uses a
fresh nonce, AES-256-GCM, and reference-bound additional authenticated data.
Headless hosts can inject an explicit standard-base64 32-byte master key. Both
paths fail closed before storage opens when their key source is unavailable.

Java streams typed row batches to Rust under bounded row and byte budgets. Rust
persists the schema and batches as four-byte big-endian length-prefixed Protobuf
frames and indexes full-frame SHA-256 hashes, offsets, ordinals, row ranges,
completion state, and expiry in SQLite. File data is synced before its index
transaction. Completed results are immutable and page reads enforce both row
and encoded-byte budgets.

Quota accounting covers the union of SQLite-indexed bytes and physical result
files, including orphan files and unindexed tails. Active writers hold
process-local leases; explicit abort, known finish failure, and ordinary drop
reclaim them immediately, while runtime expiry can reclaim abandoned writing
records. Startup removes incomplete/expired/corrupt data, truncates valid
unindexed tails, removes orphans, and rejects an unknown result format before
mutating any result.

The Stage 6 SQL executor registers `query_database` and `inspect_query_result`
for every datasource-bound run, and registers `execute_database_write` only for
`ask_before_write` runs. Reads open JDBC sessions in read-only mode. Every write
requires a fresh permission bound to the run id, tool-call id, tool name, and
SHA-256 digest of the normalized arguments; approval is consumed once before
dispatch. A durable write fence remains until Java reports a known outcome, and
unknown outcomes terminate the run without replaying the write.

Query tools receive schema, counts, truncation state, and a bounded sample
rather than appending an unbounded database result to model history. Inline
previews are capped at approximately 48 KiB, while the retained result stays in
the existing result store. The returned handle is bound to the exact session
and run, expires after 15 minutes by default, and can be paged through
`inspect_query_result` under a new explicit budget.

## Agent transport boundary

Core owns provider resolution, durable transcript restoration, serialized
state transitions, the bounded `AgentRunner`, SQL tool execution, permission
waits, terminal transcript commits, and shutdown reconciliation. Every public
Agent event is committed before it becomes visible to a subscriber. Tool
arguments are SHA-256 identified; `ToolStarted`, bounded `ToolCompleted`, and
`ToolFailed` events preserve tool identity without placing unbounded output in
the transcript. Cancellation can interrupt model or read work, but a dispatched
write waits for a known settlement or becomes an explicit unknown outcome.

Axum exposes run start, snapshot, cancellation, permission decision, and
cursor-replay SSE. Tauri exposes the same four operations plus channel
subscribe/unsubscribe, with independent subscription ids and shutdown cleanup.
The shared frontend observer deduplicates sequences, performs bounded reconnects,
and recovers from a snapshot before resuming HTTP SSE or a Tauri channel. These
are transport adapters; a complete end-user Agent workspace is not claimed by
this stage slice.

## Product transport boundary

Rust request, response, error, event, and value DTOs are the canonical external
contract. `utoipa` generates the checked-in OpenAPI document and
`openapi-typescript` generates the checked-in frontend type map; CI regenerates
both and rejects drift. JavaScript-unsafe counts, offsets, revisions,
timestamps, integer values, finite/non-finite floating values, and decimals use
portable string representations. Binary values use standard base64 and every
JDBC value is explicitly tagged.

Axum exposes secret-safe datasource CRUD, asynchronous query start, operation
snapshot/cancel, cursor-replay SSE, retained-result paging, Agent run lifecycle
and permission routes, health, product identity, and OpenAPI. Unknown `/api`
routes remain structured JSON errors even when SPA history fallback is enabled.
Loopback is the default; any non-loopback listener requires a
constant-time-checked bearer token of at least 32 bytes.

Tauri exposes the same application methods as commands. Each operation or Agent
run subscription receives its own channel after the subscription is
established; a closed Web stream or Tauri channel drops only that observer and
never implies cancellation. Desktop starts no Axum listener. Both delivery
hosts own `RuntimeHost` and shut down active operations, Agent runs, observers,
and the Java generation on exit.

The current operation journal retains at most 256 events per operation.
Subscriptions atomically capture replay plus live delivery, reject cursors
ahead of the operation or behind the retained window, and stop after one
terminal event. Query batches are durably appended before progress is emitted
or the next Java credit is granted. Failures and cancellations abort incomplete
writers and close the Java session.

The first Stage 7 compatibility slice discovers strict local driver-pack
manifests, verifies bounded artifacts in Rust, preloads them sequentially into
Java, and exposes immutable inventory through Core, Axum, Tauri, and generated
frontend contracts. A real H2 pack proves the complete product query path.
The second slice supplies the fixed Community classpath to the same Java
generation and exposes plugin catalog, schema metadata, schema SQL building,
and SQL parsing to Rust. The third slice exposes those calls through Core,
Axum, Tauri, and the shared frontend backend adapters, while enforcing the exact
embedded classpath lock at product startup and forced-read-only metadata
sessions. Signing, installation, hot reload, downloading, compatibility
selection, updates, rollback, end-user Community workflows, and the remaining
compatibility operations are not implemented.

## Local attachment and MCP boundary

Web and desktop start `LocalServer` with the same `Application` used by their
primary transport and fail startup if the local endpoint cannot be secured.
Unix uses an owner-only Unix-domain socket and peer credentials. Windows uses
an owner-only named pipe plus owner-validated endpoint metadata. Both platforms
publish a versioned endpoint record in the process-owned data directory,
authenticate each request with a random 32-byte token, and enforce bounded
length-prefixed JSON frames and I/O deadlines.

The local protocol exposes health, secret-free datasource listing,
forced-read-only query start, operation snapshot, idempotent cancellation, and
row/byte-bounded result paging. The CLI maps these operations to structured JSON
commands. It does not start another product runtime or contact Java directly.

`chat2db-mcp` uses `rmcp` 2.2 over standard stdio and maps five tools onto the
same `LocalClient`: `list_datasources`, `query_database`,
`inspect_query_operation`, `cancel_database_query`, and
`inspect_query_result`. Query start returns only an operation id. Query
retention is capped at 10,000 rows, 16 MiB, and 900 seconds; each result page is
capped at 1,000 rows and 512 KiB. Product `ApiError` values retain their stable
codes, while local paths and transport details are redacted. Stdout is
protocol-only, and dependency logging cannot be raised above `WARN` through the
MCP log setting.

This MCP slice has no write tool, Agent-run tool, or JDBC bind-parameter input.
Those capabilities are not implied by the built-in Agent's broader SQL tool
set.

## Security baseline

- Community binds to loopback by default.
- Non-loopback Web mode requires an explicit access token.
- Local CLI/MCP attachment uses an owner-only Unix-domain socket or Windows
  named pipe, owner-only endpoint metadata, per-start random authentication,
  bounded frames, and timeouts.
- Storage requires an injected, readiness-checked credential vault and never
  persists connection descriptors in SQLite or Java. Interactive hosts use an
  OS-keyring-rooted encrypted file vault; headless mode requires an explicit
  master key when no OS credential store is available.
- SQL write access is enforced outside prompts and scoped to the active run.
- User-provided driver JARs are treated as native-trust code.
- Rust copies each no-follow-opened regular driver artifact into private,
  bounded staging and verifies its SHA-256 before Java sees it.
- Java copies each staged artifact into a generation-owned private snapshot,
  verifies its SHA-256, rejects manifest `Class-Path`, and keeps it until the
  driver has no remaining session lease. Rust removes the generation root only
  after the Java child is fully reaped, preserves both primary and cleanup
  errors, and retains the snapshot when reap cannot be proven. Stale roots are
  cleared on the next storage-locked startup.
- The Community classpath is fixed application code, not a JDBC driver pack.
  Rust and Java independently reject non-canonical, symbolic-link, non-JAR, or
  over-budget entries before the isolated Community loader is created; Java
  also rejects manifest `Class-Path` escapes. The fixed build removes such
  dependency attributes deterministically before lock verification and verifies
  two consecutive clean builds byte-for-byte. Configuring it requires all four
  Community capabilities during handshake, and Community response projection
  is capped at 8 MiB in both Java and Rust. Rust applies that budget to the raw
  Community oneof values before Protobuf decoding, including duplicate fields,
  then validates decoded fields again. The source build also rejects any
  artifact-set drift against its committed lock. Signing and installed-package
  verification remain Stage 8 work.

## Packaging target

The desktop package contains the Tauri/Rust product, React assets, a private Java
compatibility JAR, a jlink-minimized Java 17 runtime, and a small signed core
driver pack. Long-tail driver packs are signed, versioned, downloaded on demand,
and independently rollback-capable.

The installed-size target is 30% to 45% below the equivalent Community package.
This is an acceptance target, not a measured current result.

Community 5.3.0 is governed by `LicenseRef-Chat2DB`, a modified Apache 2.0
license whose additional terms restrict Object-form distribution and external
product or service use without written commercial authorization. No installer,
binary archive, container image, or other Object-form release may include the
retained Community code until that authorization is recorded and release gates
have generated and verified the applicable license/NOTICE bundle and SBOM.

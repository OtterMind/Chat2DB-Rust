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

The Web and Tauri hosts open the production vault, SQLite storage, and verified
driver catalog before exposing a shared `Application`; they do not start Java
during host bootstrap. Native MySQL connection, database/schema/table, preview,
and supported Console SELECT operations do not acquire a Java lease. The Core
`EngineManager` starts one Java generation on the first JDBC-only database,
parser, formatter, completion, builder, or advanced metadata request. It shares
that single-flight startup across concurrent callers and issues generation-scoped
leases that remain live through stream and session cleanup. After the final
lease is released, the default three-minute idle deadline shuts down and fully
reaps Java. A later request starts a new generation and reloads every staged
driver pack. Axum serves JSON, SSE, and
the exact pinned original Community Umi/React SPA; Tauri exposes commands and
per-subscription channels without a localhost product server. Both hosts also publish an owner-only local endpoint
for the CLI and MCP process. That same `Application` owns query and Agent run
execution, replay, cancellation, and write-permission decisions. Strict local
managed driver packs and immutable inventory are implemented. A fixed Community
5.3.0 submodule now supplies a real H2 compatibility slice for plugin discovery,
schema, relational object, relation, and programmability metadata, dialect SQL
building, and retained ANTLR parsing. Product Core, Axum, Tauri, and both
frontend backend adapters expose catalog, schemas, databases, tables, columns,
indexes, views, imported and exported foreign keys, primary keys, functions,
function parameters, procedures, procedure parameters, triggers, schema SQL
building, parsing, syntax validation, formatting, and datasource-aware SQL
completion plus datasource-free typed DML, namespace SQL, and bounded
table-preview SQL generation when the exact locked classpath is configured.
The current product frontend is not the former repository-owned replacement
workbench. The build exports the unmodified Community frontend tree pinned by
`scripts/community-frontend.lock.json`. Web maps its historical `/api`
contract through Axum; desktop maps the existing `window.javaQuery` contract
through one Tauri `legacy_request` command. Both paths call the same Rust
legacy dispatcher. The implemented product slice covers native MySQL connection
testing, datasource CRUD/tree, database/schema/table discovery, read-only table
preview, and typed Console SELECT. Signing, distribution, the remaining dialect
estate, broader historical API coverage, and packaging remain target
components. CLI and MCP attach to a running host rather than composing a
second product runtime.

Runtime-tested: yes for the Stage 7M MySQL product vertical. On 2026-07-27 the
complete stored-datasource path passed against MySQL 8.4, from real Community
identifier/DQL/page-limit generation through forced-read-only JDBC execution
and retained-result paging. On 2026-07-29 the native MySQL path passed against
MySQL 8.4 with a deliberately missing Java executable, including active-query
cancellation and dormant-Java assertions after every product operation. The
complete repository `make verify` gate and an explicit real-MySQL rerun passed
after preserving the disabled-engine error contract.

## Ownership

| Area | Owner | Boundary |
| --- | --- | --- |
| Shared UI | Original Community Umi, React, and TypeScript | Historical HTTP on Web and `window.javaQuery` over Tauri IPC on desktop |
| Desktop | Rust / Tauri 2 | Tauri commands and events; no product localhost HTTP |
| Web | Rust / Axum | JSON HTTP and SSE |
| Product services | Rust | Workspace, state, policy, tasks, dashboards, and orchestration |
| Durable state | Rust | SQLite, retained-result files, and a mandatory injected secret-vault contract |
| AI agent | Rust | Provider adapters, tool loop, limits, compaction, and cancellation |
| MCP and CLI | Rust | Adapters around the same product services and policy |
| Native MySQL product slice | Rust / `mysql_async` | Connection, first-stage read-only object metadata and Community-compatible legacy routes/envelopes, nullable defaults, preview, supported SELECT, typed streaming, limits, cancellation, and retained results |
| Compatibility databases and advanced MySQL operations | Java 17 | Existing SPI/plugins, JDBC, SQL builders, parsing, formatting, completion, writes, transactions, and non-MySQL metadata |
| SQL parsing, formatting, and completion | Java 17 | Existing Java ANTLR grammars, parser behavior, formatter behavior, and completion |
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
              -> native MySQL connection / metadata / SELECT
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
- A Java-backed operation must retain its `EngineLease` until every session,
  stream, transaction, and cleanup action for that operation is terminal.

The implemented lifecycle and JDBC subset is documented in
[`protocol.md`](protocol.md). Capabilities are advertised only after their
cross-language acceptance gates pass.

## Database boundary

Java/JDBC remains the compatibility implementation for other databases and for
unmigrated advanced MySQL operations. The first closed native route uses
upstream `mysql_async 0.37.0` for MySQL connection testing,
database/schema/table discovery, preview, and supported Console SELECT. Core
selects this backend before requesting an `EngineLease`; unrecognized drivers
cannot enter it. Parameterized, CTE-first, locking, server-file, and
multi-statement SELECT is rejected rather than silently starting Java.

The native MySQL baseline implements:

- JDBC-URL and connection-property translation into `mysql_async::Opts`, with
  explicit Rustls policy, TCP preference, and a 15-second connect deadline;
- database/schema/table metadata and safely quoted bounded table preview;
- one read-only prepared SELECT with ordered typed columns and values emitted
  through the existing retained-result wire contract;
- row, result-byte, batch, column, SQL, identifier, and scalar limits; and
- active-query cancellation and truncation through a second bounded connection
  issuing `KILL CONNECTION`, followed by deterministic result cleanup.

The JDBC baseline implements:

- verified external JAR snapshots and per-driver classloader isolation;
- JDBC session and local-transaction ownership;
- prepared query and update execution with typed parameters;
- typed row batches with row, byte, frame, and scalar limits;
- credit flow control, cancellation, deadlines, and conservative outcomes.

Stage 7B additionally implements:

- a Git submodule fixed at Community commit
  `37a34be858f2566b6b7fcf6c3f64183c1f560853`;
- a reproducible H2 compatibility classpath, established with 148 JARs and
  extended in Stage 7J to 149 JARs for the retained Community domain-core
  completion implementation, whose filenames, byte lengths, and SHA-256
  digests are bound to that commit by the checked-in
  `third_party/community-h2-classpath.lock`;
- deterministic build-time removal of dependency-manifest `Class-Path` entries,
  with affected JARs rebuilt as sorted, ZIP-precision commit-timestamped `STORED`
  archives;
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
and 149 filenames, byte lengths, and SHA-256 digests come only from the lock
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

Stage 7D extends the same boundary with the independent
`community.metadata.objects.v1` capability. Java invokes the selected real
`IDbMetaData.databases`, `tables`, `columns`, and `indexes` methods and projects
only compatibility-owned DTOs. Core exposes four matching services; every call
resolves the vault-backed datasource, forces a read-only JDBC session, and uses
the same cancellation-safe close path. Axum adds four POST routes, Tauri adds
four commands, and the shared HTTP/Tauri frontend adapter keeps identical
request and response types. Database, table, column, index, and cumulative
index-column counts are explicit, while both Java and Rust enforce the shared
8 MiB response ceiling.

Stage 7E adds the independent `community.metadata.relations.v1` capability.
Java invokes real `IDbMetaData.views`, `getImportedKeys`, `getExportedKeys`, and
`getPrimaryKeys` methods. Core exposes four forced-read-only,
cancellation-safe services; Axum adds four POST routes; Tauri adds four
commands; and both frontend adapters share the generated request and response
types. Views reuse the stable table projection. View, foreign-key, and
primary-key lists each have explicit 65,536-entry limits, and Rust applies both
allocation-free pre-decode counts and decoded response validation under the
same cumulative 8 MiB ceiling. The real H2 gates create a view plus named
primary and foreign keys and verify both foreign-key directions through the
bridge and product boundary.

Stage 7F adds the independent `community.metadata.programmability.v1`
capability. Java invokes the real function, function-parameter, procedure,
procedure-parameter, and trigger metadata methods through eight protocol
operations. Core gives every operation a forced-read-only,
cancellation-safe datasource session; Axum adds eight POST routes; Tauri adds
eight commands; and both frontend adapters share the generated request and
response types. Each repeated collection is capped at 65,536 entries. Rust
pre-decode scanning covers raw tags `212..=219`, then decoded validation applies
the same field, aggregate, and 8 MiB response limits. The H2 adapter compensates
for the retained plugin's schema-in-`databaseName` detail lookup while requiring
the requested catalog to match the active connection, escaping H2 SQL literal
values, restoring verified external identities, and converting empty detail
projections to stable not-found errors. Real bridge and product gates create
Java aliases and a trigger, exercise every operation plus injection-shaped and
missing-detail requests, and prove driver unload after all metadata sessions
close.

Stage 7G connects the complete fixed 20-operation Community product contract to
the shared React workbench. Its three-pane layout keeps datasource selection,
Community objects, and the SQL/result workspace visible together. Plugin,
database, and schema scopes lazily load table, view, function, procedure, and
trigger groups, followed by relational or programmability details only after
selection. Independent requests publish each settled result without waiting for
slower peers, so an unsupported long-tail metadata group cannot hide successful
groups. One refresh action
retries catalog and scope failures while preserving the selected scope. The
initial plugin is mapped from the managed JDBC driver's class, and a missing
driver identity never guesses a dialect. Direct routine and trigger lookup
covers objects omitted by a vendor's list metadata. `CREATE SCHEMA` inserts
generated SQL without executing it, and the bounded Analyze action is enabled
only when the selected plugin advertises parser support. Scope and detail
requests are abortable; the responsive object browser preserves keyboard
tab/dialog behavior and starts collapsed on mobile.

Stage 7H adds the independent `community.sql-validation.v1` capability without
changing the existing parser operation. Java invokes the retained Community
`ISQLParser.parserStatements` path and projects statement summaries plus source
diagnostics through Protobuf tag `220`. Each result is capped at 4,096
diagnostics; Java and Rust enforce coordinate, string, aggregate, encoded-size,
and cumulative 8 MiB response limits, while Rust also counts raw nested
diagnostics before Protobuf allocation. Validation is parser-only and never
opens a JDBC session. Core, Axum, Tauri, and both frontend adapters expose one
matching request/response contract, and the React editor provides an explicit
parser-gated Validate action alongside Analyze. Real H2 bridge and product
tests cover both valid and invalid SQL.

Stage 7I adds the independent `community.sql-formatter.v1` capability at
Protobuf request/response tag `221`. Java calls the retained
`com.github.vertical-blank:sql-formatter:2.0.4` implementation with Community's
database-type mapping: MySQL, PostgreSQL, PL/SQL, T-SQL, DB2, and MariaDB use
their matching dialects, while every other type uses the generic formatter.
Formatter exceptions preserve the original SQL. Rust limits both input and
output SQL to 1 MiB. Rust and Java apply a shared 16,384-unit linear lexical
complexity preflight before the retained formatter, preventing token-dense
input from exhausting the 30-second generation request deadline. Rust rejects
duplicate raw response payloads before Protobuf allocation and retains the
cumulative 8 MiB Community response ceiling.
Formatting resolves neither datasource secrets nor JDBC sessions. Core, Axum,
Tauri, and both frontend adapters expose the same request/response contract;
the editor replaces its SQL only if the originating SQL, datasource, and
database type remain current. Real H2 bridge and product tests cover the
generic fallback through the fixed Community classpath.

Stage 7J adds the independent `community.sql-completion.v1` capability at
Protobuf request/response tag `222`. The fixed classpath grows from 148 to 149
JARs by adding Community domain-core. The compatibility adapter reflectively
invokes `DbSqlCompletionServiceImpl.complete` rather than reimplementing
completion policy: MySQL reaches `DefaultSqlSyntaxHandler`, while other
relational database types reach `GenericSqlCompletionEngine`, with metadata
resolved through the already-open JDBC connection. The adapter attaches that
connection to a temporary `ConnectInfo`, supplies `IDbTableService.queryColumns`
through a dynamic proxy, clears only the private Community thread-local and the
per-request `MemoryCacheManage` entries, and verifies that completion did not
close the external connection.

The product datasource id never enters the Java wire contract. Rust assigns a
fresh non-zero `datasource_scope` to isolate Community's process-global
completion cache, while Core supplies the stored datasource display name and
owns a forced-read-only, cancellation-safe session. Cursor positions, global
and candidate replacement ranges, snippet ranges, and editor columns use UTF-16
units so Rust, Java, and the browser agree even for non-BMP text. Before
Protobuf allocation, Rust accumulates duplicate tag-`222` payloads under the
shared 8 MiB budget and caps 4,096 candidates, 4,096 editor hints, 65,536 hint
items, and 65,536 snippet slots. Decoded validation reapplies collection,
string, enum, range, semantic, and encoded-size limits.

Axum exposes `POST /api/v1/community/sql/complete`, Tauri exposes
`complete_community_sql`, and generated OpenAPI/TypeScript plus the shared
HTTP/Tauri adapters use one product-owned contract. The React editor requests
completion explicitly, discards responses after SQL, cursor, datasource,
database/schema scope, or refresh changes, and applies a candidate only through
a valid UTF-16 edit range. Real H2 bridge and product gates cover table
completion after `select * from ` and `ID`/`LABEL` column completion after
`select items. from APP.items` through the fixed classpath and read-only product
session.

Stage 7K adds the independent `community.dml-builder.v1` capability at
Protobuf request/response tag `223`. The adapter invokes the selected real
plugin's DML builder, value processor, and identifier processor without opening
a JDBC session. Its process-neutral contract contains independently quoted
database/schema/table/column segments and a closed typed-value union, with no
raw SQL or expression variant. It supports single and batch INSERT plus UPDATE
with non-empty ordered equality predicates; DELETE, UPSERT, DEFAULT, functions,
and arbitrary operators remain outside the contract.

Core exposes datasource-free SQL generation, Axum publishes
`POST /api/v1/community/sql/build-dml`, Tauri publishes
`build_community_dml`, and both frontend adapters use the generated shared DTOs.
The table detail opens a modal editor for selected columns, multiple INSERT
rows, explicit NULL, and separate UPDATE SET/WHERE selections. Generated SQL is
inserted into the existing editor and is never executed automatically. Abort
identity covers close, table switch, and refresh so late responses cannot write
into a newer editor scope. Real bridge and product gates generate first, prove
the database is unchanged, then independently execute and read back typed H2
values.

Stage 7L adds `community.namespace-builder.v1` at tag `224`. Its closed request
union covers database create/alter/drop/use and schema create/alter/drop without
accepting raw SQL. Java invokes the real plugin-owned database or schema DDL
builder without opening a datasource session; Rust enforces raw and decoded
budgets, and Core, Axum, Tauri, OpenAPI/TypeScript, and React expose the same
generated-SQL-only contract. The old CREATE SCHEMA operation remains available.
Real H2 bridge/product gates prove generation is non-executing, while the fixed
Java classpath gate verifies both H2 and MySQL builder output.

Stage 7M adds `community.dql-builder.v1` at request/response tag `225`. The
catalog advertises `dql_builder_available` per plugin. Java treats database,
schema, and table names as separate bounded identifier segments, uses the
selected plugin's real identifier builder to produce the qualified name,
then calls its DQL `buildSelectTable` and `buildPageLimit` implementations. Some
plugins, including MySQL, quote the table argument again; the adapter detects
that incompatible first result, retries the same real builder with the original
segments, and requires the resulting SELECT to contain the exact plugin-quoted
qualified identifier. This generation step accepts no raw SQL, opens no JDBC
session, and never executes the returned statement.

Core applies the product default of 200 rows and rejects limits outside
`1..=1000`. The current MySQL route safely quotes the database and table as two
identifier segments and builds the bounded SELECT directly in Rust. Other
drivers retain the Community builder and parser checks for `is_select`, one
projected statement, a SELECT prefix, and no semicolon. Both routes pass the SQL
to `start_read_query`, which dispatches MySQL to a native read-only transaction
and other drivers to a forced-read-only JDBC session. It caps the result at the
same row limit and 8 MiB, writes batches of at most 1 MiB, and retains the result
for one hour. The accepted response carries the operation id, exact SQL, and
effective row limit; normal operation events, cancellation, and retained-result
paging remain unchanged.

Axum exposes `POST /api/v1/community/table-preview`; Tauri exposes
`start_community_table_preview`; generated OpenAPI/TypeScript and both frontend
adapters share the same DTO. The table detail enables Preview only when the
selected plugin advertises DQL support. Starting a preview inserts the generated
SQL into the editor and observes the accepted operation in the existing result
surface. A table/scope change aborts the pending request, and a late accepted
operation is cancelled instead of replacing newer state. The Core path is
runtime-tested against MySQL 8.4 through both the historical Connector/J gate
and the native `mysql_async` gate. Product writes
and Agent, CLI, and MCP MySQL conformance remain outside Stage 7M; PostgreSQL and
long-tail plugin conformance do not block this MySQL milestone.

Remaining builder operations, complete MySQL type conformance, native bind
parameters and CTE-first SELECT, non-relational
operations, script execution, import/export, and per-dialect conformance are not
implemented yet.

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

Java/JDBC streams typed row batches to Rust under bounded row and byte budgets;
the native MySQL producer emits the same wire column, row-batch, and completion
messages directly in Core. Rust
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
or the next Java credit/native row is consumed. Failures and cancellations abort
incomplete writers and close the Java session or terminate the native MySQL
connection through a separate bounded control connection.

The first Stage 7 compatibility slice discovers strict local driver-pack
manifests, verifies bounded artifacts in Rust, preloads them sequentially into
Java, and exposes immutable inventory through Core, Axum, Tauri, and generated
frontend contracts. A real H2 pack proves the complete product query path.
The second slice supplies the fixed Community classpath to the same Java
generation and exposes plugin catalog, schema metadata, schema SQL building,
and SQL parsing to Rust. The third slice exposes those calls through Core,
Axum, Tauri, and the shared frontend backend adapters, while enforcing the exact
embedded classpath lock at product startup and forced-read-only metadata
sessions. The fourth slice adds databases, tables, columns, and indexes across
the same Java/Core/Axum/Tauri/frontend path with independent capability
negotiation and bounded responses. The fifth slice adds views, both foreign-key
directions, and primary keys through another independent capability. The sixth
slice adds functions, procedures, their parameters, and triggers through eight
more product operations. The seventh slice connects all 20 operations to the
shared React workbench through an object explorer, lazy details, partial-result
handling, schema SQL insertion, and explicit SQL analysis. The eighth and ninth
slices add independently negotiated SQL validation and formatting. The tenth
slice calls Community's retained completion service against the existing
read-only JDBC session and exposes bounded UTF-16 suggestions through the same
Core/Axum/Tauri/React product boundary. The eleventh slice generates typed
single/batch INSERT and ordered UPDATE SQL through the real plugin-owned DML,
value, and identifier processors and inserts it into the editor without
execution. The twelfth slice adds closed database/schema namespace DDL
generation through the same product boundary. The thirteenth slice generates a
bounded table SELECT through the real plugin-owned identifier, DQL, and
page-limit builders, then executes it through Core's forced-read-only query and
retained-result path for Web and Tauri. Commit `928e62c` supersedes the custom
product UI from those intermediate slices with the exact original Community
frontend while retaining the Rust capabilities behind explicit historical API
adapters. Signing,
installation, hot reload, downloading, compatibility selection, updates,
rollback, and the remaining compatibility operations are not implemented.

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
  two consecutive clean builds byte-for-byte. Configuring it requires all
  thirteen Community capabilities during handshake, and Community response projection
  is capped at 8 MiB in both Java and Rust. Rust applies that budget to raw
  Community response tags `200..=225` before Protobuf decoding, including
  duplicate fields, and allocation-free scans known nested repeated fields so
  collection limits are enforced before DTO allocation. It then validates
  decoded collection counts, nested index columns, completion fields, typed-DML,
  namespace SQL, table-preview SQL and row limits, UTF-16 ranges, field sizes,
  aggregate strings, and encoded message length again. The source build also
  rejects any artifact-set drift against its committed lock.
  Signing and installed-package verification remain Stage 8 work.

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

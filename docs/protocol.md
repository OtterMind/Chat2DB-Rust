# Compatibility Process Protocol

## Status

Implemented and cross-language tested: protocol 1.0 framing, handshake, exact
version selection, required-capability validation, lifecycle supervision,
external JDBC driver loading, sessions, local transactions, prepared queries
and updates, typed row streams, credit flow control, cancellation, deadlines,
bounded errors, conservative delivery outcomes, Community plugin inventory,
H2 schema, database, table, column, index, view, imported/exported foreign-key,
primary-key, function, function-parameter, procedure, procedure-parameter, and
trigger metadata, typed DML and database/schema namespace SQL building, retained
SQL parsing, bounded syntax validation, SQL formatting, and datasource-aware SQL
completion.

Not implemented in the compatibility protocol: general-purpose SQL builder
operations, script execution, data import/export, non-relational operations, or
retained result handles. Driver-pack discovery
and inventory are product-level Rust contracts; the process protocol continues
to receive only ordered canonical JDBC JAR paths and expected digests.

## Source of truth

`proto/chat2db/compat/v1/compat.proto` and its imported `jdbc.proto` and
`community.proto` are the canonical wire schema. Rust generates types in
`chat2db-engine-protocol` with `prost-build`; Maven generates Java from the same
files with Protobuf 4.31.1. Generated sources stay in build directories and are
not committed.

The retained Community SPI and its implementations come from the Community
5.3.0 submodule fixed at commit
`f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7`. The Protobuf messages are
compatibility-layer-owned DTOs, not serialized Community Java types; Community
plugin, JDBC, parser, and exception objects remain inside Java. The catalog's
`source_commit` is provenance that Rust checks against the configured commit.
It is not an artifact digest, cryptographic signature, or trust decision.

## Framing

Every stdin/stdout message is:

```text
4-byte unsigned big-endian payload length
Protobuf payload bytes
```

The negotiated receive limit must be between 1 KiB and the 16 MiB hard payload
limit, inclusive. Each sender applies the peer's advertised limit to every
response, including a rejected handshake, and checks the encoded size before
allocating the serialized payload. The two directions are independent: a
host's advertised receive limit does not reduce the engine's receive limit.
Empty, oversized, truncated, and malformed frames are fatal. A clean EOF is
recognized only between frames. Java stdout is reserved for frames in protocol
mode; diagnostics are written to stderr.

## Handshake

The first request must be `ClientHello`. It carries the runtime identity, the
exact versions the host supports, required capability strings, and its receive
limit. Java selects the highest exact common version and returns `ServerHello`
with a unique engine instance id, selected version, capabilities, and receive
limit.

The implemented capability names are:

- `lifecycle.ping.v1`
- `lifecycle.shutdown.v1`
- `driver.external-jar.v1`
- `session.jdbc.v1`
- `query.typed-batches.v1`
- `flow.credit.v1`
- `operation.cancel.v1`
- `update.jdbc.v1`
- `transaction.local.v1`
- `community.plugin-catalog.v1`
- `community.metadata.schemas.v1`
- `community.metadata.objects.v1`
- `community.metadata.relations.v1`
- `community.metadata.programmability.v1`
- `community.sql-builder.v1`
- `community.sql-parser.v1`
- `community.sql-validation.v1`
- `community.sql-formatter.v1`
- `community.sql-completion.v1`
- `community.dml-builder.v1`
- `community.namespace-builder.v1`

No common version or missing required capability returns a fatal structured
error and terminates the engine. Rust rejects a selected version it did not
offer and rejects an incomplete engine identity.

Every startup failure signals termination and joins the actor before returning.
If startup and actor or snapshot cleanup both fail, the bridge preserves both
errors. A generation snapshot is deleted only after the child is confirmed
reaped; an uncertain terminate/reap result returns an explicit process-cleanup
error and retains the canonical snapshot path for later cleanup.

## Correlation and outcomes

Every request carries request, trace, optional session, deadline, and optional
cancellation identity. Correlation ids are non-empty and at most 255 UTF-8
bytes. Every response echoes request and trace identity and carries a stream
sequence and terminal flag. Unary calls produce one terminal response with
sequence zero. Query streams start at sequence zero and remain contiguous
through their terminal response.

Rust registers a request before it enters the single writer queue and routes
responses by request id, so responses may arrive out of order. Timed-out ids
enter a bounded retired set so one late terminal response does not corrupt the
next request. Unknown, duplicate, or trace-mismatched responses fail the
generation.

A request rejected before the writer queue is `NotSent`. A delivered JDBC
unary or credit request whose response is abandoned or times out makes the
process generation fatal because its resource or session effect can no longer
be reconciled. Lifecycle ping retains the bounded late-terminal retirement
path. A broken pipe or crash is conservatively `Unknown`. Nothing is
automatically replayed.

The actor updates shared session state when a wire response is routed, before a
consumer polls the corresponding future or stream. An abandoned stream can
therefore still move its session to `ROLLBACK_REQUIRED` or `BROKEN`.

## JDBC drivers and sessions

Rust sends an ordered list of private, host-staged JAR paths and expected
SHA-256 digests. Java copies each artifact into a generation-owned private
snapshot while hashing it, rejects a digest mismatch and any manifest
`Class-Path`, then loads the requested
`java.sql.Driver` through a `URLClassLoader` whose parent is the platform
classloader. The requested class must come from that loader. Java does not
download a driver and the shaded engine JAR contains no H2 classes.

The generated hard limits allow at most 32 artifacts, 256 MiB per artifact,
and 1 GiB across one load request. Java counts copied bytes while writing the
snapshot, so a source that grows after Rust validation is still bounded and
rejected.

The engine owns the stable driver id:

```text
"sha256:" + lower_hex(SHA-256(
  "chat2db-jdbc-driver-v1\0" || utf8(driver_class) || "\0" ||
  artifact_sha256[0] || artifact_sha256[1] || ...
))
```

For `org.h2.Driver` and the ordered digest bytes `00..1f`, the fixed vector is
`sha256:7668f940329b5cbd3854e8692e92bd944405d41361d79e98fea7998bbe47d720`.
Rust recomputes the id and verifies the artifact count returned by Java.

An opaque session owns one JDBC connection and at most one active operation.
Its explicit state is `AUTO_COMMIT`, `TRANSACTION_ACTIVE`,
`ROLLBACK_REQUIRED`, `BROKEN`, or `CLOSED`. Local transaction ids must be bound
to every query or update while a transaction is active. Session close rolls
back an active transaction. If close cannot prove that the connection closed,
the driver lease remains owned so cleanup can be retried safely.

## Queries, updates, and flow control

`QueryStarted` is sequence zero and carries bounded column metadata without
consuming credit. Every `RowBatch` consumes exactly one credit and respects the
requested row and byte targets plus the peer frame limit. `QueryCompleted`
reports rows actually emitted. Hitting `max_rows` or `max_result_bytes` exactly
is not truncation unless Java observes an additional database row.

An initial or incremental credit grant is at most 8 batches and total
outstanding credit is at most 32. Java reserves credit before advancing or
reading the `ResultSet`. With a forward-only cursor, byte packing may carry one
already-read row, bounded by the target batch bytes, into the next batch; no
additional JDBC read occurs without another credit.

The default total result budget is 64 MiB when `max_result_bytes` is zero and
the hard maximum is 1 GiB. Other hard limits include 4,096 rows per batch, 8
MiB per batch, 4 MiB per scalar value, and 2,048 columns. Rust validates both
outbound requests and inbound streams against the generated limits, requested
budgets, and negotiated peer frame size.

Values preserve null, boolean, signed and unsigned integer, decimal text,
floating point, text, bytes, temporal, JSON, UUID, and opaque database values.
Java reads large text, binary, LOB, SQLXML, and opaque values through bounded
streams. Sensitive connection-property values are redacted through a bounded
scanner before error text crosses the wire.

Query registration occurs synchronously on the protocol thread before the
worker is submitted, so immediate credit and cancel requests can find it.
For a normally settled query or update, the single protocol writer retires the
operation and releases its session ownership immediately before writing the
first byte of the terminal frame. A request sent after observing that terminal
frame therefore cannot collide with the completed operation, while its response
still remains ordered behind the terminal frame on the wire.
Deadlines have a watchdog and cancellation uses a separate bounded worker. If
driver cancellation does not settle before the terminal timeout, Java returns
`UNKNOWN` with a `BROKEN` session but retains the ResultSet, Statement,
connection, and driver lease until the cancel worker actually exits.

An update is considered attempted immediately before the JDBC execute call.
Every checked or unchecked failure after that point, including an invalid
negative update count or statement-close failure, reports outcome `UNKNOWN`.
Transactions and writes are never automatically replayed.

## Community compatibility

Rust accepts a Community classpath only with a full lowercase source commit,
at most 256 non-symbolic regular JARs, and at most 512 MiB in total. It opens
every artifact without following links, records its length and SHA-256, then
copies and re-verifies it into the Java generation's private directory. The
parent process's Community environment variables are removed before the
validated directory and commit are supplied to the child. Java independently
requires one canonical directory containing only bounded, readable JARs and
loads it through a `URLClassLoader` whose parent is the platform classloader.
Java rejects every JAR whose manifest declares a non-empty `Class-Path`, so the
loader cannot resolve an artifact outside the generation snapshot.
The fixed source build removes dependency-manifest `Class-Path` attributes
before lock verification by rebuilding only affected JARs with sorted entries,
the source-commit timestamp, and the `STORED` method. A two-clean-build gate
compares every resulting JAR digest and length.

Configuring the classpath automatically makes all twelve Community capabilities
required during handshake. A sidecar that lacks any one of them fails startup
and is reaped before the generation becomes ready.

`ServiceLoader<IPlugin>` discovery and every Community invocation run with that
loader as the thread context loader. Catalog responses project plugin identity,
database/schema behavior, declared JDBC configuration, and available metadata,
SQL builder, parser, DML builder, value processor, and identifier processor
services into bounded Protobuf DTOs. Java stops projection
when cumulative UTF-8 plus conservative encoding accounting exceeds 8 MiB;
Rust independently scans the undecoded `ServerEnvelope` wire and accumulates
the raw length-delimited values of every Community response field, including
unknown nested bytes and duplicate oneof fields. The same allocation-free scan
parses known Community submessages and rejects plugin, driver, download URL,
schema, statement, object, nested index-column, function, function-parameter,
procedure, procedure-parameter, trigger, completion-candidate, editor-hint,
hint-item, and snippet-slot counts before Protobuf DTO decoding can allocate
their repeated collections. More than 8 MiB is fatal
while non-Community responses retain the negotiated limit up to 16 MiB.
Decoded counts, string totals, and message sizes are checked again against the
same generated limits. Rust requires the catalog's source commit to equal the
configured commit; a mismatch is a fatal protocol violation.

The object-metadata capability adds four unary session-bound operations:
`ListCommunityDatabases`, `ListCommunityTables`, `ListCommunityColumns`, and
`ListCommunityIndexes`. Java invokes the selected plugin's real
`IDbMetaData.databases`, `tables`, `columns`, and `indexes` methods. The
corresponding server-envelope tags are `204..=207`; tags `200..=203` retain
their existing catalog, schema, builder, and parser meanings. Database lists
are capped at 4,096 entries; table, column, and index lists at 65,536 entries
each; and index columns plus foreign-column names at 65,536 cumulatively across
one response. Scalar, comment, SQL, aggregate-string, encoded-message, and raw
wire budgets are checked independently. These are direct SPI lists: for
example, Community's default H2 implementation leaves derived column
`primary_key` and index `type` values unset, while the corresponding unique
index and indexed columns remain available from `ListCommunityIndexes`. The
bridge keeps nullable JDBC `Long` fields as Protobuf `int64`; Core converts table
increment, row, and data-length values plus index cardinality, page, and prefix
length values to nullable decimal strings before the Axum/Tauri/TypeScript
boundary so JavaScript cannot lose integer precision.

The relation-metadata capability adds four more unary session-bound operations:
`ListCommunityViews`, `ListCommunityImportedKeys`,
`ListCommunityExportedKeys`, and `ListCommunityPrimaryKeys`. Java invokes the
selected plugin's real `IDbMetaData.views`, `getImportedKeys`,
`getExportedKeys`, and `getPrimaryKeys` methods. Their server-envelope tags are
`208..=211`; all existing `200..=207` meanings remain unchanged. Views reuse
the bounded `CommunityTable` projection. View lists, each foreign-key list, and
each primary-key list are capped at 65,536 entries. Both sides validate scalar
and aggregate response budgets, and Rust counts these repeated fields from raw
wire bytes before Protobuf decoding and validates them again afterward.

The programmability-metadata capability adds eight unary session-bound
operations: function list/detail/parameters, procedure list/detail/parameters,
and trigger list/detail. Java invokes the selected plugin's real
`IDbMetaData.functions`, `function`, `getFunctionParameters`, `procedures`,
`procedure`, `getProcedureParameters`, `triggers`, and `trigger` methods. Their
server-envelope tags are `212..=219`; all existing `200..=211` meanings remain
unchanged. Function, function-parameter, procedure, procedure-parameter, and
trigger collections are independently capped at 65,536 entries. Both sides
validate scalar, aggregate-string, encoded-message, and 8 MiB cumulative
response budgets, while Rust counts each repeated field from raw wire bytes
before Protobuf decoding and validates it again afterward. Community 5.3.0's
H2 detail implementation uses request `databaseName` as its schema predicate;
the compatibility adapter first requires the requested catalog to equal the
active JDBC catalog, SQL-literal-escapes the H2 schema and object names, supplies
the schema for that internal lookup, and restores the verified catalog plus
original identifiers in the external DTO. Empty H2 detail projections become
stable not-found errors rather than caller-labelled placeholder objects. H2
exposes Java aliases through JDBC procedure-list metadata even when its
information schema classifies an alias as a function, so the fixed H2
conformance test expects an empty function list and still verifies function
detail directly.

Schema, object, relation, and programmability metadata reuse an existing
generation-bound JDBC session and optional transaction. Java validates each
request and selected plugin before claiming the session operation. Any
`RuntimeFailure`, unchecked failure, or linkage failure after the claim
conservatively marks an active transaction `ROLLBACK_REQUIRED`. A claim-stage
`NOT_STARTED` outcome is promoted to
`KNOWN_FAILED`, while an existing `UNKNOWN` outcome remains unknown; failures
rejected before the claim leave the transaction active and retain
`NOT_STARTED`.
`CREATE SCHEMA` uses the SQL builder owned by the plugin's `IDbMetaData`;
parsing uses the plugin's retained syntax parser. No Community, JDBC, ANTLR, or
exception object crosses the process boundary.

The SQL-completion capability adds one unary, session-bound operation at client
and server envelope tag `222`. Its request carries the database type and scope,
SQL, cursor, prefix policy, keyword case, optional active snippet slot, and a
non-zero bridge-generated `datasource_scope`. Product datasource ids never enter
the wire request. All cursor, global replacement, candidate replacement, active
snippet, and editor-range columns are JavaScript/Java UTF-16 units. Rust
validates outbound offsets against the SQL before dispatch and validates every
returned range against the same SQL after response routing.

The fixed classpath includes Community domain-core so Java can reflectively
construct and invoke `DbSqlCompletionServiceImpl.complete`. The adapter attaches
the existing session connection to a temporary `ConnectInfo`; MySQL reaches
`DefaultSqlSyntaxHandler`, while other relational types reach
`GenericSqlCompletionEngine`. A dynamic `IDbTableService` proxy services column
lookups through the retained JDBC metadata path. Request cleanup removes only
the adapter's private thread-local and `MemoryCacheManage` scope; it does not call
the Community context helper that would close the Rust-owned connection. The
adapter verifies the connection remains open when completion returns.

Completion responses carry status, a default replacement range, candidates,
editor hints, and an optional reason code. Rust's allocation-free raw scanner
accumulates every tag-`222` payload, including duplicate oneof values, under the
8 MiB Community budget and caps candidates and editor hints at 4,096 each and
hint items and snippet slots at 65,536 each. Decoded routing validates status and
enumerated values, scalar and aggregate strings, paired and ordered replacement
ranges, one-based editor positions, collection counts, and encoded size before
the bridge converts the result to its public type.

The typed-DML capability adds one unary, datasource-free operation at client and
server envelope tag `223`. The request selects a database type, carries raw
database/schema/table identifier segments, and contains either one INSERT row,
multiple INSERT rows, or ordered UPDATE assignments plus non-empty ordered
equality predicates. Values are a closed union of NULL, string, plain decimal,
boolean, ISO-8601 temporal, and binary bytes. There is no raw SQL, expression,
function, DEFAULT, DELETE, UPSERT, or predicate-operator input.

Java invokes the selected plugin's real `IPlugin.getSqlBuilder().dml()`,
`ValueProcessor.getSqlValueString`, and
`SQLIdentifierProcessor.quoteIdentifier` implementations. It independently
quotes each qualified-name segment, validates each typed value against its
declared column type and the rendered literal, and returns SQL only. It opens no
JDBC session and never executes the generated statement. Rust and Java cap one
request at 2,048 columns, 4,096 rows, 32,768 values, 8 MiB encoded bytes, and
bounded identifier, type-name, decimal, temporal, string, and binary fields.
The returned SQL is non-empty and at most 1 MiB.

The namespace capability adds one unary, datasource-free operation at envelope
tag `224`. Its closed oneof supports create/alter/drop/use database and
create/alter/drop schema operations. Database and schema models contain bounded
identifier, comment, charset, collation, and owner fields; no variant accepts
raw SQL. Java calls the selected plugin's real metadata-owned
`ddl().database()` or `ddl().schema()` builder, opens no JDBC session, and
returns SQL without executing it. Unsafe identifier, property, and comment
syntax is rejected before reflection. The rendered SQL is non-empty and at most
1 MiB, and the old tag-`202` CREATE SCHEMA operation remains compatible.

All twenty-five Community operations use the fatal-on-unknown lane. Once
delivered, a timeout or abandoned response terminates and reaps the Java
generation because the host can no longer prove the plugin invocation's state.
The checked-in
`third_party/community-h2-classpath.lock` additionally binds the current
source build to exactly 149 filenames, lengths, and SHA-256 values. This lock is
a reproducibility and drift gate, not a package signature or distribution
authorization.

The product runtime embeds that lock and accepts only a directory matching it
exactly. `CHAT2DB_COMMUNITY_CLASSPATH_DIR` selects the directory but cannot
override its source commit or inventory. Core maps all twenty-five protocol
operations to stable external DTOs; schema, object, relation, and
programmability metadata plus completion resolve a vault-backed datasource and
use a forced-read-only JDBC session before invoking this protocol. Completion
also injects the stored datasource display name; only Rust's private
`datasource_scope`, never the product id, crosses the Java boundary. Core keeps
each bounded session operation alive if its transport waiter is cancelled so it
can consume the response and close the session. These product rules sit above
the compatibility wire contract. Typed-DML and namespace generation remain
datasource-free. Axum exposes `POST /api/v1/community/sql/build-dml` and
`POST /api/v1/community/sql/build-namespace`; Tauri exposes
`build_community_dml` and `build_community_namespace_sql`. Both transports
return the same generated SQL DTO.

## Supervision

One actor owns each process generation. It coordinates a bounded stdin writer,
a stdout response reader, a 64 KiB stderr tail drain, child termination, and all
pending requests. Observable states are `Starting`, `Handshaking`, `Ready`,
`Stopping`, `Stopped`, `Failed`, and `Crashed`.

Graceful shutdown sends `Shutdown`, waits for `ShutdownAck`, closes stdin, and
reaps the child. A deadline breach force-kills and still reaps the process. A
crash fails every pending request with unknown outcome. The current bridge does
not restart the process automatically.

## Verification

`make verify` runs strict Rust workspace formatting and Clippy, all Rust unit,
contract, process-supervisor, and documentation tests, the full Java test
suite, packaged Rust-to-Java lifecycle and JDBC integration, real H2 product
tests, generated-contract drift checks, frontend typechecking/tests/build, and
desktop checks. The H2 tests cover external driver loading, managed-pack
preload and cleanup, typed multi-batch streaming, zero-credit backpressure,
transactions, rollback on close, cancellation recovery, and retained results.
CI runs the same cross-language H2 gates without conditional skips and adds a
Windows managed-pack runtime test. The Stage 7B gate performs a clean build of
the fixed Community source with a commit-derived archive timestamp and a
repository-local Maven cache, rejects classpath lock drift, keeps the H2 JDBC
driver external, and executes catalog, schema, object, relation,
programmability, schema-builder, and ANTLR-parser calls through the real
Community H2 plugin. CI runs that same Community sidecar path on Linux and
Windows. The Stage 7C-7L product gate starts from the exact embedded lock,
stores an encrypted H2 datasource, invokes the fixed operations through Core,
and verifies database/table/column/index, view, foreign-key, primary-key,
function, procedure, parameter, and trigger projection plus metadata-session
and driver cleanup. Its completion cases verify table suggestions after
`select * from ` and `ID`/`LABEL` column suggestions after
`select items. from APP.items`, using H2's actual JDBC catalog and the same
forced-read-only product-session boundary. Its DML cases prove single and batch
INSERT plus ordered UPDATE generation, prove generation does not execute SQL,
then independently execute and read back typed H2 values.
Namespace cases likewise generate H2 CREATE/DROP SCHEMA SQL, prove the database
is unchanged, and only then execute and verify each state transition. The Java
fixed-classpath gate also verifies the real MySQL CREATE DATABASE builder.
The separate MySQL preview gate loads the pinned Connector/J pack and verifies
real database/table/column/index metadata, read-query execution, and retained
result paging through Core. It does not expand the protocol with a product
write surface or claim Agent, CLI, or MCP MySQL conformance.

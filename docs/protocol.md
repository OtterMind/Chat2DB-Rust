# Compatibility Process Protocol

## Status

Implemented and cross-language tested: protocol 1.0 framing, handshake, exact
version selection, required-capability validation, lifecycle supervision,
external JDBC driver loading, sessions, local transactions, prepared queries
and updates, typed row streams, credit flow control, cancellation, deadlines,
bounded errors, and conservative delivery outcomes.

Not implemented: Chat2DB plugin and driver-pack inventory, generic database
metadata operations, script execution, import/export, SQL parser or completion
operations, retained result handles, or product transport wiring. Database
product information returned while opening a session is the only metadata in
the Stage 3 subset.

## Source of truth

`proto/chat2db/compat/v1/compat.proto` and its imported `jdbc.proto` are the
canonical wire schema. Rust generates types in `chat2db-engine-protocol` with
`prost-build`; Maven generates Java from the same files with Protobuf 4.31.1.
Generated sources stay in build directories and are not committed.

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

No common version or missing required capability returns a fatal structured
error and terminates the engine. Rust rejects a selected version it did not
offer and rejects an incomplete engine identity.

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

Rust sends an ordered list of external JAR paths and expected SHA-256 digests.
Java copies each artifact into a private snapshot while hashing it, rejects a
digest mismatch and any manifest `Class-Path`, then loads the requested
`java.sql.Driver` through a `URLClassLoader` whose parent is the platform
classloader. The requested class must come from that loader. Java does not
download a driver and the shaded engine JAR contains no H2 classes.

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

`make verify` runs the Rust workspace checks, 12 protocol contract tests, 22
bridge unit tests, 36 process-supervisor tests, 54 Java unit tests, two packaged
Java integration tests, three Rust-to-Java lifecycle/crash tests, three real H2
JDBC tests, and the frontend production build. The H2 tests cover external
driver loading, typed multi-batch streaming, zero-credit backpressure,
transactions, rollback on close, and cancellation recovery. CI runs the same
cross-language H2 gate without a conditional skip.

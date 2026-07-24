# Compatibility Process Protocol

## Status

Implemented and cross-language tested: protocol 1.0 framing, handshake, exact
version selection, required-capability validation, ping, structured errors,
graceful shutdown, bounded stderr capture, timeout handling, and crash
classification.

Not implemented in this protocol version: JDBC driver loading, connections,
transactions, SQL execution, row streams, cancellation, metadata, or parser
operations. Those operations enter the schema only with their stage tests.

## Source of truth

`proto/chat2db/compat/v1/compat.proto` is the only wire schema. Rust generates
types in `chat2db-engine-protocol` with `prost-build`; Maven generates Java from
the same file with Protobuf 4.31.1. Generated sources stay in build directories
and are not committed.

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

No common version or missing required capability returns a fatal structured
error and terminates the engine. Rust rejects a selected version it did not
offer and rejects an incomplete engine identity.

## Correlation and outcomes

Every request carries request, trace, optional session, deadline, and optional
cancellation identity. Every response echoes request and trace identity and
carries a stream sequence and terminal flag. Stage 2 lifecycle calls produce a
single terminal response with sequence zero.

Rust registers a request before it enters the single writer queue and routes
responses by request id, so responses may arrive out of order. Timed-out ids
enter a bounded retired set so one late terminal response does not corrupt the
next request. Unknown, duplicate, or trace-mismatched responses fail the
generation.

A request rejected before the writer queue is `NotSent`. Once accepted into the
process path, a timeout, broken pipe, or crash is conservatively `Unknown`.
Nothing is automatically replayed.

## Supervision

One actor owns each process generation. It coordinates a bounded stdin writer,
a stdout response reader, a 64 KiB stderr tail drain, child termination, and all
pending requests. Observable states are `Starting`, `Handshaking`, `Ready`,
`Stopping`, `Stopped`, `Failed`, and `Crashed`.

Graceful shutdown sends `Shutdown`, waits for `ShutdownAck`, closes stdin, and
reaps the child. A deadline breach force-kills and still reaps the process. A
crash fails every pending request with unknown outcome; Stage 2 does not restart
the process automatically.

## Verification

`make verify` runs Rust frame tests, a dedicated process fixture, Java unit and
packaged-JAR tests, and three real Rust-to-Java tests: normal lifecycle,
incompatible protocol, and external process kill. CI runs the cross-language
gate in its own job with Rust and Java installed together.

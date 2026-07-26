# Staged Delivery

Every stage ends in a focused commit and must leave `main` buildable. A stage is
complete only after its local gates pass; planned capabilities remain explicit
in runtime health until then.

| Stage | Status | Deliverable | Required evidence |
| --- | --- | --- | --- |
| 1 | Complete | Repository baseline | Rust format/Clippy/tests, Java tests/package, frontend typecheck/build, CI workflow |
| 2 | Complete | Rust-Java process protocol | Generated Protobuf in both languages, handshake, ping, version negotiation, stderr capture, process shutdown and crash tests |
| 3 | Complete | JDBC vertical slice | Dynamic H2 driver load, session lifecycle, typed query batches, backpressure, cancellation, transaction semantics, Rust integration tests |
| 4 | Complete | Product and result storage foundation | SQLite migration/integrity gates, mandatory vault boundary, revisioned datasource records, durable result frames, bounded paging/quota, expiry, writer cleanup and recovery tests |
| 5 | Complete | Product transports | Generated OpenAPI/TypeScript contract, Axum JSON/SSE, Tauri 2 commands/channels, shared SQL workbench, product H2 tests |
| 6 | Complete | Agent, MCP, and CLI | Direct providers, durable bounded tool loop, SQL tools/permissions, compaction, Web/Tauri run transports, owner-only local attachment, read-query CLI, and bounded `rmcp` stdio tools |
| 7 | In progress | Chat2DB compatibility estate | 7A managed JDBC packs, 7B fixed Community H2 SPI/ANTLR, 7C product Core/Web/Tauri contracts, 7D relational object metadata, 7E relation metadata, 7F programmability metadata, and 7G end-user object explorer implemented; remaining plugin operations and per-dialect conformance explicit |
| 8 | Planned | Packaging and release | License authorization, NOTICE/SBOM, jlink runtime, Tauri installers, signed product/engine/driver manifests, atomic update and rollback, size measurement |

Stage 3 completion means the versioned Rust-Java bridge can load an external
JDBC driver, own sessions and local transactions, execute updates, and stream
typed query batches under explicit limits, credits, deadlines, and
cancellation. Stage 5 composes that bridge into the Web and desktop product
hosts. Stage 6 adds CLI and MCP adapters that attach to one of those running
hosts and do not own another product runtime.

Stage 4 completion means `chat2db-storage` owns a process-locked SQLite schema,
datasource revisions and secret references, persistent secret-cleanup intents,
and immutable completed result files indexed by full-frame hashes. Result pages
have row and encoded-byte limits, quota accounting includes indexed and physical
files, active writers hold leases, and startup rejects unknown result formats
before mutation. Stage 5 adds the production vault adapters and composes storage
into Web and desktop; Stage 6 exposes a bounded subset through local attachment.

Stage 5 completion means Rust DTOs generate a checked-in OpenAPI document and
TypeScript types, one React SQL workbench selects either an HTTP/SSE or Tauri
command/channel adapter, and both transports expose the same datasource, query,
operation, cancellation, and retained-result services. Query submission is
asynchronous, event replay is cursor-based and bounded, disconnecting a stream
does not cancel work, and Web API fallbacks remain JSON rather than SPA HTML.
The Web and desktop bootstrap paths fail closed unless durable storage, a
production secret vault, and the Java engine start successfully. Real H2 product
tests cover secret-backed datasource execution, retained paging, explicit
cancellation, and Java session release. Managed driver-pack installation and
the existing Chat2DB plugin/ANTLR compatibility estate remain Stage 7, so the
production hosts currently require drivers to be provisioned by a future driver
manager; H2 is loaded only by the product integration fixture.

Stage 6 provides direct OpenAI, Anthropic, and Gemini
adapters; a provider-neutral bounded Agent loop; durable session, message, run,
permission, and compaction state; and Core-owned start, snapshot, replay,
cancellation, terminal commit, and shutdown behavior. Datasource-bound runs can
query through a forced read-only JDBC session, inspect retained results through
run-bound expiring handles, and execute a write only after a fresh approval
bound to the exact tool call and argument digest. Axum exposes the lifecycle as
JSON plus replay/live SSE; Tauri exposes matching commands and independent
channels; the frontend has matching HTTP/Tauri observers with bounded recovery.

Web and desktop start the same owner-only local attachment around their shared
`Application`. The JSON CLI exposes health, datasource listing,
forced-read-only query start/status/cancel, and bounded retained-result pages.
The `rmcp` stdio server exposes the matching five datasource/query lifecycle
tools, returns only an operation id from query start, and requires result
polling and paging. MCP retention is capped at 10,000 rows, 16 MiB, and 900
seconds; pages are capped at 1,000 rows and 512 KiB. The current MCP contract is
read-only and accepts no JDBC bind parameters; it does not claim the built-in
Agent's write tool.

Stage 7A implements strict local JDBC driver-pack discovery, bounded artifact
hashing, startup preload, and immutable inventory through Core, Axum, Tauri,
and generated frontend contracts. Downloading, signing, installation, update,
rollback, and hot reload remain incomplete.

Stage 7B fixes the Community source at commit
`f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7`, builds its H2 compatibility
classpath reproducibly, and locks all 148 JAR filenames, lengths, and SHA-256
digests. Before lock verification, the fixed build strips dependency-manifest
`Class-Path` entries deterministically and proves two clean builds have
identical artifact bytes. Rust snapshots and re-verifies those JARs for one
supervised Java generation. Java isolates them behind a platform-parent
`URLClassLoader`, rejects manifest `Class-Path` escapes, discovers real `IPlugin`
services, and exposes plugin catalog, H2 schema metadata, `CREATE SCHEMA`, and
retained ANTLR parsing DTOs over Protobuf. Configuring the classpath requires all
seven current Community capabilities at handshake, and Java plus Rust enforce the
generated 8 MiB cumulative response budget. Rust counts raw Community oneof
values before decoding, including duplicate fields, and retains a generation
snapshot whenever child reap cannot be proven. The real vertical test keeps the
H2 JDBC driver in its separate driver loader.

Stage 7C embeds that exact 148-JAR lock in the product Core and allows Web or
desktop startup to opt into it through `CHAT2DB_COMMUNITY_CLASSPATH_DIR`. Any
missing, extra, renamed, symbolic-link, length-drifted, or digest-drifted entry
fails startup before Java launches. Core projects the four compatibility calls
into stable product DTOs, resolves schema metadata through encrypted datasource
storage, and opens that metadata session in forced-read-only mode. Axum exposes
four generated-contract routes; Tauri and the shared frontend backend client
expose the same operations. Health reports the compatibility component as
ready, disabled, or unavailable from both configuration and negotiated engine
state. Metadata work continues in a bounded Core task after a transport waiter
is cancelled, so the JDBC session still reaches its explicit close path. A real
H2 product test covers locked startup, catalog, encrypted datasource resolution,
builder execution, metadata, parser, session cleanup, and driver unload on
Linux and Windows CI.

Stage 7D adds the independent `community.metadata.objects.v1` capability while
preserving the existing schema capability and wire field numbers. Java invokes
the real `IDbMetaData.databases`, `tables`, `columns`, and `indexes` methods and
projects compatibility-owned DTOs under explicit limits: 4,096 databases;
65,536 tables, columns, and indexes; 65,536 cumulative index columns and foreign
column names; and an 8 MiB total response. Rust enforces raw wire tags
`204..=207` before decode, including allocation-free repeated-field counts, and
validates decoded counts, field sizes, aggregate strings, nested index columns,
and encoded length again. Core exposes four forced-read-only, cancellation-safe
metadata services and converts nullable 64-bit metadata integers to decimal
strings at the JavaScript boundary; Axum, Tauri, and the shared HTTP/Tauri
frontend backend expose matching contracts. Real H2 bridge and product tests
verify the current catalog, created table and columns, primary index, and custom
unique index through the exact fixed 148-JAR classpath.

Stage 7E adds the independent `community.metadata.relations.v1` capability and
preserves every existing wire field number by assigning `208..=211` to views,
imported keys, exported keys, and primary keys. Java invokes the real
`IDbMetaData.views`, `getImportedKeys`, `getExportedKeys`, and `getPrimaryKeys`
methods. Rust applies allocation-free repeated-field limits before Protobuf
decode and validates decoded fields, collection sizes, aggregate strings, and
encoded bytes again. Core exposes four forced-read-only, cancellation-safe
services; Axum, Tauri, and the shared HTTP/Tauri frontend backend expose the
same generated contracts. Real H2 bridge and product gates create a view,
named primary key, and named foreign key, verify both foreign-key directions,
and prove metadata-session cleanup still permits driver unload.

Stage 7F adds the independent `community.metadata.programmability.v1`
capability and preserves all existing wire meanings by assigning `212..=219`
to function list/detail/parameters, procedure list/detail/parameters, and
trigger list/detail responses. Java invokes the real Community metadata SPI and
projects compatibility-owned DTOs; each list or parameter collection is capped
at 65,536 entries under the cumulative 8 MiB response budget. Rust counts these
repeated fields directly from the undecoded wire before allocation, then
validates decoded counts, fields, aggregate strings, and encoded bytes again.
Core exposes eight forced-read-only, cancellation-safe services; Axum, Tauri,
and the shared HTTP/Tauri frontend backend expose matching contracts. Real H2
bridge and product gates create Java aliases plus a trigger, verify H2's JDBC
procedure-list behavior, exercise all eight services, preserve the external
catalog across H2's schema-based detail lookup, and prove session cleanup still
permits driver unload.

Stage 7G connects the complete fixed 20-operation Community product contract to
the shared React workbench. The three-pane layout keeps datasource selection,
Community objects, and SQL/results visible together. Plugin, database, and
schema scopes drive lazy table, view, function, procedure, and trigger lists;
table details load columns, indexes, imported/exported keys, and primary keys;
routine and trigger details load only after selection. Independent metadata
groups publish as they settle and retain successful results when a long-tail
plugin lacks or delays another group. The heading refresh action retries catalog
and scope failures without discarding the selected database or schema.
Driver-class mapping chooses the initial plugin, while a missing inventory
identity never guesses a dialect. Direct name lookup keeps H2 aliases reachable
even when its function list is empty. `CREATE SCHEMA` builds SQL into the editor
without executing it, and SQL parsing is an explicit bounded Analyze action
enabled only when the selected plugin advertises parser support. Focused model
tests verify request scope, operation fan-out, progressive and partial result
retention, abort propagation, deterministic selection, and driver-to-plugin
mapping.

Stage 7 remains incomplete. Type conversion, script execution, data
import/export, formatting, validation, completion, non-relational behavior,
remaining builders and plugin inventory, driver distribution, and per-dialect
conformance are not implemented.

Before Stage 8 may produce any Object-form distribution containing Community
5.3.0 code, the release must record written commercial authorization compatible
with `LicenseRef-Chat2DB`. It must also generate and verify the complete
license/NOTICE attribution bundle and SBOM for the exact locked artifacts.
Signing, installed-package verification, atomic update/rollback, and measured
installed-size acceptance remain mandatory independent gates. A private source
build passing Stage 7 does not satisfy these release conditions.

## Commit policy

- One semantic commit per completed stage or independently reviewable stage
  slice.
- No generated output without its source and reproducible generation command.
- No disabled test, placeholder success response, or capability claim used to
  make a stage appear complete.
- Schema and protocol compatibility changes include fixtures from the prior
  version.
- Every database write/cancellation/crash path includes an unknown-outcome test.

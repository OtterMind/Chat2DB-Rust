# MySQL Community Parity Contract

## Status

- Community baseline: `OtterMind/Chat2DB`
  `main@3cb8af54cad5bd5caa20bb25f10d9b0e4f01931c`.
- Rust milestone: Issue `#14`, branch
  `feat/mysql-community-complete-parity`.
- Implementation: complete in the milestone working tree for every MySQL
  workbench capability reached by the pinned Community frontend.
- Runtime-tested: yes, with local MySQL 8.4 for native metadata, Console,
  editable data, DDL, routines, transfer, account administration, schema diff,
  Dashboard/Chart refresh, views, workspace state, Web HTTP, and
  Desktop-compatible dispatch. These native paths keep Java dormant.
- SSH status: implemented and runtime-tested through a temporary Docker SSH
  endpoint without enabling macOS Remote Login. Two concurrent MySQL queries
  shared one datasource/revision-scoped fixed-port tunnel; the final lease
  released the listener, the fixture was removed, and Java remained dormant.
- Release status: the local repository, Community frontend, Java/H2, real
  MySQL, and real SSH gates pass in the milestone working tree. After the
  Dashboard/Chart increment, the complete `rtk make verify` gate and its Rust,
  process, Java, IPC/JDBC/H2, frontend, and Desktop checks also pass. The normal
  public CI workflow must pass on the staged commits before Issue `#14` is
  closed.

This file is the acceptance contract for MySQL work. A Core capability counts
only when the original Community UI can reach it through both Axum HTTP and the
Tauri legacy bridge. Host-specific transport patches are allowed only when the
locked frontend commit, tree, and production build reproduce them.

## Ownership

- `mysql_async 0.37.0` owns native MySQL connections, metadata, queries,
  updates, transactions, cancellation, account administration, schema diff,
  chart refresh, and transfer.
- Rust owns the Axum/Tauri product host, SQLite workspace/task/dashboard/chart
  state, encrypted datasource secrets, SSH tunnels, MyBatis Plus class
  generation, AI Agent, CLI, and MCP.
- The fixed Community Java compatibility process owns only parser, formatter,
  completion, SQL-builder, and plugin behavior where exact Community semantics
  are required. It starts on demand and has no HTTP port.
- The original Community pages, components, interaction model, and styles are
  retained. A locked host-adapter patch changes only transport-facing source
  for CSP-safe callbacks and Web/Desktop file upload and download behavior.

## Capability Matrix

| Area | Community contract | Milestone status |
| --- | --- | --- |
| Runtime bootstrap | `/api/system`, `/api/common/environment`, `/api/jdbc/driver/list` | Implemented for Web and Desktop with the native MySQL driver inventory. |
| Datasource CRUD and lifecycle | datasource list/get/create/update/delete, clone, connect, close, console connect, grouping | Implemented with revision CAS, safe edit projections, secret-preserving empty-password updates, clone/close semantics, and namespace persistence. Create responses do not echo submitted connection data; edit/list responses return only sanitized URL, username, non-sensitive properties, and `readOnly`. |
| SSH and driver lifecycle | `/api/connection/ssh/pre_connect`, datasource `ssh`, JDBC driver download/upload/save/delete | Implemented. Password/private-key SSH settings persist inside the encrypted datasource descriptor; passwords and passphrases never leave the vault. Native MySQL callers share managed tunnel leases rather than bypassing SSH in metadata or Console paths. |
| Datasource portability | converter upload routes, Community import/export, Navicat, DBeaver, DataGrip | Implemented for Chat2DB JSON, Navicat NCX v11/v12, DBeaver DBP/AES credentials, and DataGrip text, with bounded ZIP/XML/file parsing and secret-safe export. |
| Database and schema | list, create SQL, create, confirmed delete, metadata projections | Implemented natively with pagination, system flags, charset/collation/comment fields, and two-phase destructive confirmation. |
| Tables and metadata | table/list/query/meta, columns, indexes, primary/imported/exported keys | Implemented natively. Composite keys, nullable defaults, generated/invisible safety checks, `UNSIGNED`/`ZEROFILL`, ENUM/SET values, and editor ordering are covered. |
| Editable data | preview, count, insert/update/delete SQL and execution, copy SQL/IN values | Implemented with bounded reads, PK-first optimistic writes, explicit nulls, Community result envelopes, and large-cell retention. |
| Table DDL | create/alter/drop/truncate/copy, example aliases, `SHOW CREATE`, export | Implemented for the pinned MySQL editor surface. Foreign-key mutation is not exposed by that editor; foreign-key metadata remains available to ER and schema diff. |
| Views | list/columns/detail/query/meta/create-or-replace/drop | Implemented with the six-field Community editor projection and real MySQL execution. |
| Functions, procedures, triggers | list/detail/parameters, invocation preview, migration preview/execute | Implemented. Migration replacement uses compensating restore behavior; generated invocations preserve modes, order, defaults, and quoted identifiers. |
| Console | `/api/rdb/dml/execute`, DDL/update aliases, Desktop SQL stream/cancel | Implemented for SELECT/CTE, DDL/DML, scripts, custom `DELIMITER`, transactions, `EXPLAIN`, paging, multiple result sets, cancellation, error-continue, read-only enforcement, durable history, and bounded retained results. The pinned Community request has no write-bind field; typed SELECT binds are an additional Rust capability. |
| Large values | cell preview/read/download/path | Implemented with owner-scoped expiring tokens, UTF-8 byte/character boundaries, Base64/hex modes, and bounded fallback previews. |
| Import, export, and tasks | SQL/CSV/XLS/XLSX import/export, task list/get/stop/download, DML export, class generation | Implemented with durable tasks, cancellation/recovery, streamed Web attachments, Desktop paths, and generated SQL ZIPs. Rust renders MyBatis Plus entity, Mapper, and Mapper XML files from native MySQL metadata, writing local files for Desktop or a bounded ZIP artifact for Web. |
| Account administration | capability/list/grants/preview/execute | Implemented for seven actions, three scopes, and fourteen privileges, with preview-token authorization, escaping, password redaction, and read-only rejection. |
| Structure comparison | `/api/diff/sql` | Implemented as read-only source-to-target preview. It covers tables, columns/order, primary and secondary indexes, foreign keys, engine/charset/collation/comment, and views. DDL is target-qualified, foreign keys are retargeted, dependent views are topologically ordered, and the output is real-MySQL tested to converge. Runtime AUTO_INCREMENT counters are intentionally not treated as schema. |
| Workspace state | saved consoles/history, namespaces, pins, ER metadata/positions | Implemented in SQLite with restart-safe ownership and migration coverage. |
| Dashboards and charts | Dashboard/Chart CRUD plus chart detail refresh | Implemented through all ten historical Web/Tauri routes. Dashboard and chart documents persist in SQLite; refresh reads the chart's `databaseInfo` and executes native MySQL under the boundary below. |
| SQL compatibility | parser, formatter, validation, keywords, context parser/tip/hover/completion | Every historical route is mapped to the fixed Community Java implementation with shared Web/Desktop envelopes and lazy Java startup. |
| AI workspace | `/api/v3/ai/chat/stream`, history, model list/options/config/test, attachments | Implemented as a compatibility facade over the Rust Agent. Web SSE and Desktop `ai_sse_message` are mapped; model secrets are retained safely; legacy runs are forced read-only and unexpected write approval requests are denied. |
| Agent, CLI, and MCP writes | Rust Agent tools, CLI write, MCP write tool | Implemented through native `mysql_async` with explicit confirmation, single-statement validation, datasource read-only enforcement, prepared-protocol dispatch, and `not_started` versus `unknown` retry semantics. Non-MySQL writes never fall back to Java. |

## Dashboard and Chart Boundary

The original Community frontend reaches Dashboard/Chart through the same Rust
legacy dispatcher on Web and desktop:

| Method | Historical route | Operation |
| --- | --- | --- |
| `GET` | `/api/dashboard/list` | Search and page dashboards. |
| `GET` | `/api/dashboard` | Load one dashboard. |
| `DELETE` | `/api/dashboard` | Delete one dashboard. |
| `POST` | `/api/dashboard/create` | Create one dashboard. |
| `POST` | `/api/dashboard/update` | Partially update one dashboard. |
| `GET` | `/api/v1/chart` | Load one persisted chart without SQL execution. |
| `GET` | `/api/chart/detail` | Load a chart and optionally refresh its query result. |
| `POST` | `/api/v1/chart/create` | Create one chart. |
| `POST` | `/api/v1/chart/update` | Partially update one chart. |
| `DELETE` | `/api/chart` | Delete one chart. |

SQLite migration 8 stores dashboards and charts, including chart schema,
persisted metadata, database context, refresh settings, and dashboard chart-id
relations. A `refresh=false` detail request performs no database query. For a
refresh, Rust reads `dataSourceId`, `sql`, `databaseName`, `schemaName`, and
`consoleId` from `databaseInfo`, selects the requested database, and accepts
exactly one parsed MySQL `SELECT` or SELECT CTE. Writes, multiple statements,
locking reads, and `INTO OUTFILE`/`DUMPFILE` are rejected before dispatch.

Accepted SQL runs through native `mysql_async` inside `START TRANSACTION READ
ONLY` and is rolled back before disconnect. The response is page 1 capped at
200 rows and 8 MiB. `dataList` contains string-or-null cells and `headerList`
uses the Community column shape. Simple single-table results enrich headers
from native MySQL column metadata, including primary key, auto-increment,
integer nullability, default, comment, size, scale, and editor type. Refreshed
`metaData` exists only on the detached response and never replaces the chart's
persisted `metaData`. Successful and rejected refreshes write durable
`SQL_EXECUTE` history with `extendInfo.source = "CHART"`, chart id, and console
id. The complete path is native and does not acquire a Java lease.

## Schema Diff Boundary

The pinned Community Liquibase comparison surface does not expose independent
CHECK-constraint editing, partition editing, or MySQL runtime counters. The
Rust diff therefore does not claim those as editable parity. It fails closed on
case-only object conflicts under case-insensitive MySQL naming and on cyclic
view dependencies instead of emitting ambiguous SQL.

## Verification Gates

Before merge, the milestone requires:

1. Rust format, workspace tests, all-target/all-feature checks, and strict
   Clippy with locked dependencies.
2. Locked Community frontend source verification, typecheck, tests, and
   production build without UI/component/style changes; the host transport
   patch must be committed and reproducible.
3. Web and Desktop contract tests, including AI SSE and legacy dispatch.
4. Real MySQL 8.4 product tests for Core Console, metadata, transfer, accounts,
   schema diff, Dashboard/Chart refresh, and the Web editable/DDL vertical, plus
   a real SSH-forwarded concurrent-query test with Java dormancy and fixture
   cleanup assertions.
5. Knowledge-base lint and staged commits followed by the normal public CI
   workflow. The macOS package workflow remains manual-only and is not invoked
   by this milestone.

## Source Anchors

- `third_party/chat2db-community/chat2db-community-client/src/service/`
- `third_party/chat2db-community/chat2db-community-server/chat2db-community-web/src/main/java/ai/chat2db/community/web/api/controller/`
- `apps/chat2db-web/src/legacy.rs`
- `apps/chat2db-web/src/legacy_ai.rs`
- `apps/chat2db-desktop/src/lib.rs`
- `apps/chat2db-desktop/src/legacy_files.rs`
- `crates/chat2db-contract/src/`
- `crates/chat2db-contract/src/community_dashboard.rs`
- `crates/chat2db-core/src/native_mysql.rs`
- `crates/chat2db-core/src/mysql_dashboard.rs`
- `crates/chat2db-core/src/mysql_account.rs`
- `crates/chat2db-core/src/mysql_schema_diff.rs`
- `crates/chat2db-core/src/mysql_workspace.rs`
- `crates/chat2db-core/src/ssh.rs`
- `crates/chat2db-core/src/transfer/`
- `crates/chat2db-storage/migrations/005_workspace_namespace.sql`
- `crates/chat2db-storage/migrations/006_transfer.sql`
- `crates/chat2db-storage/migrations/007_mysql_workspace.sql`
- `crates/chat2db-storage/migrations/008_community_dashboard.sql`
- `crates/chat2db-storage/src/community_dashboard.rs`
- `apps/chat2db-web/tests/native_mysql_editable_ddl_docker.rs`
- `crates/chat2db-core/tests/native_mysql_product.rs`
- `crates/chat2db-core/tests/native_mysql_transfer_docker.rs`
- `crates/chat2db-core/tests/native_mysql_account_docker.rs`
- `crates/chat2db-core/tests/native_mysql_schema_diff_docker.rs`
- `crates/chat2db-core/tests/native_mysql_dashboard_docker.rs`
- `crates/chat2db-core/tests/native_mysql_ssh_tunnel_docker.rs`
- `java/compat-runtime/src/main/java/ai/chat2db/rust/compat/CommunityH2IdentifierCompatibility.java`

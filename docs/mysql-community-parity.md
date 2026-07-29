# MySQL Community Parity Contract

## Status

- Community baseline: `OtterMind/Chat2DB` `main@3cb8af54cad5bd5caa20bb25f10d9b0e4f01931c`.
- Rust baseline: `OtterMind/Chat2DB-Rust` `main@e9789cded5d227ba1f48690264046610eedb0807`.
- Product target: the original Community React frontend running against the Rust Web or Tauri host.
- Runtime-tested now: datasource CRUD/test; database, schema, table, column,
  index, foreign-key, primary-key, view, function, procedure, trigger, and
  routine-parameter metadata; all first-stage historical metadata routes;
  bounded table preview; saved Console CRUD; and native unparameterized MySQL
  Console execution. Console coverage includes DDL/DML, semicolon scripts,
  `DELIMITER` procedure scripts, explicit transactions, error-continue policy,
  preserved-single dispatch, `EXPLAIN`, bounded all-row paging, datasource
  read-only enforcement, multiple result sets, exact affected-row counts,
  cancellation, a shared 64 MiB retained-result budget, durable history with
  cancelled-state projection, and bounded large-cell retrieval/download. Base64
  and hex large-text chunks use UTF-8 byte offsets as the original frontend
  expects. Native metadata and Console execution keep Java dormant. Paged table
  name/comment search, complete-list filtering, page-size validation, HTTP
  binding-error envelopes, and nullable column defaults match the locked
  Community baseline.
- Complete parity: not implemented.

This file is the acceptance contract for MySQL work. Community frontend routes
and user-visible behavior define parity. A modern Core, Axum, Tauri, Java, or
native MySQL capability does not count as complete until the original frontend
route reaches it and a real MySQL product test covers the behavior.

## Ownership

- `mysql_async` owns MySQL connections, metadata, query/update execution,
  transactions, cancellation, large values, and data transfer.
- The fixed Community Java compatibility process retains the exact Community
  ANTLR parser, formatter, completion engine, and plugin SQL builders where
  reproducing their behavior in Rust would create unnecessary divergence.
- Rust remains the only product host. Java has no HTTP port and starts only for
  compatibility operations that require it.
- The original Community frontend and its styles remain unchanged. Compatibility
  is implemented behind its existing HTTP and `window.javaQuery` contracts.

## Capability Matrix

| Area | Community frontend contract | Rust baseline | Required parity |
| --- | --- | --- | --- |
| Runtime bootstrap | `/api/system`, `/api/common/environment`, `/api/jdbc/driver/list` | Implemented | Preserve exact envelopes and immutable driver inventory. |
| Datasource CRUD and test | `/api/connection/datasource/list`, `/datasource`, `/datasource/create`, `/datasource/pre_connect`, `/datasource/update`, `DELETE /datasource` | Implemented | Keep secret-safe persistence and native MySQL connection testing. |
| Datasource lifecycle | `/api/connection/datasource/connect`, `/datasource/close`, `/connection/close`, `/connection/console/connect`, `/datasource/clone` | Not implemented | Match explicit connect/close/clone behavior and frontend refresh semantics. |
| SSH and JDBC driver management | `/api/connection/ssh/pre_connect`, `/api/jdbc/driver/download`, `/upload`, `/save`, `/delete` | Not implemented | Match Community SSH testing and local driver lifecycle without exposing secrets. |
| Datasource import/export and namespaces | converter upload routes, `/api/connection/datasource/import_community`, `/datasource/export`, `/api/namespaces/*` | Not implemented | Support Community, Chat2DB, Navicat, DBeaver, DataGrip, export, grouping, and ordering. |
| Database and schema metadata | `/api/rdb/database/list`, `/database_schema_list`, `/api/rdb/schema/list` | Database/schema list implemented | Match filtering, system flags, charset/collation, comments, and pagination envelopes. |
| Database and schema mutation | database create/modify/delete and `/api/rdb/delete/{database,schema}/{prepare,execute}` | Builder-only modern contracts | Generate previews through Community builders and execute only after the same explicit frontend command. |
| Table inventory and detail | `/api/rdb/table/list`, `/table_list`, `/table_meta`, `/column_list`, `/index_list`, `/key_list`, `/query` | List, compact list, column, index, and key routes implemented with native MySQL metadata; nullable defaults and legacy envelopes match Community; `table_meta` and `/query` remain | Add the remaining table-meta and query projections without regressing the native routes. |
| Table data operations | `/api/rdb/dml/execute_table`, `/execute_update`, `/get_update_sql`, `/copy_update_sql`, `/copy_in_values_sql`, `/count` | Read-only preview only; closed typed DML generation exists behind modern APIs | Support editable rows, insert/update/delete SQL, counts, copy helpers, optimistic predicates, and bounded execution. |
| Table DDL | `/api/rdb/ddl/*`, `/api/rdb/table/modify/sql`, `/delete`, `/truncate`, `/copy`, create/update examples, DDL export | Partial builder infrastructure only | Match create/alter/drop/truncate/copy preview and execution, including columns, indexes, keys, charset, collation, comments, and MySQL types. |
| Views | `/api/rdb/view/list`, `/column_list`, `/detail`, `/query`, `/view_meta`, `/modify/sql`, `/delete`, `/drop` | Native list, column list, and `SHOW CREATE VIEW` detail are mapped to the legacy routes | Add data query, metadata, preview, create/alter, and drop flows. |
| Functions, procedures, and triggers | `/api/rdb/{function,procedure,trigger}/{list,detail}`, `/api/rdb/routine/{preview_invocation,preview_migration,execute_migration}` | Native list/detail and routine-parameter projections are implemented; every original list/detail route is mapped | Add invocation and migration preview/execution flows. |
| Console SELECT | `/api/rdb/dml/execute`, desktop `sql-execute`/`sql-cancel` | Native unparameterized MySQL reads, CTEs, normal/all-row paging, preserved-single dispatch, `EXPLAIN`, limits, multiple result sets, affected-row counts, datasource read-only enforcement, and cancellation implemented | Add JDBC-style bind parameters and close remaining warning/error/result-shape differences. |
| Console scripts and writes | `/api/rdb/dml/execute`, `/execute_ddl` | Native unparameterized DDL/DML, semicolon and `DELIMITER` scripts, explicit transactions, error-continue policy, cancellation, and per-statement results implemented | Add bind parameters and complete exact Community conformance for unsupported edge-case scripts. |
| Large cell values | `/api/rdb/cell/value`, `/download`, `/download_path` | Bounded UTF-8/Base64 previews, owner-scoped expiring tokens, byte-oriented Base64/hex chunk reads, character-oriented text reads, and full-value downloads implemented | Add long-running export/task integration and close remaining content-type/display-mode differences. |
| Saved consoles and SQL history | `/api/operation/saved/*`, `/api/operation/log/{create,list}` and detail | Restart-safe saved Console CRUD plus durable history create/list/detail, filtering, paging, per-statement recording, and cancelled-state projection implemented | Complete remaining Community audit/history fields and non-Console producers. |
| SQL parser, formatter, validation, completion | `/api/sql/format`, `/valid_select`, `/api/sql_parser/get_keywords`, `/context/{parser,quick_parser,tip,hover}` | Modern parser/validation/formatter/completion contracts implemented through Java; legacy routes absent | Map every original endpoint to the fixed Community implementation with matching UTF-16 offsets and envelopes. |
| Import, export, and tasks | `/api/import/{sql_file,other_file}`, `/api/export/{sql_file,other_file}`, `/api/task/*`, `/api/rdb/dml/export`, table class generation | Not implemented | Add bounded streaming import/export, progress, stop, download, cleanup, and failure recovery. |
| Account administration | `/api/rdb/account/{capability,list,grants,preview,execute}` | Not implemented | Match MySQL users, hosts, authentication, privileges, role/grant previews, execution, and current Community escaping rules. |
| Structure comparison | `/api/diff/sql` | Not implemented | Match Community structure projection and MySQL synchronization SQL without changing shared query parsing behavior. |
| Pins and ER metadata | `/api/pin/table/*`, `/api/er/*` | Not implemented | Persist pinned tables and expose the metadata needed by the existing ER view. |
| AI, CLI, and MCP | Original `/api/ai` UI plus Community CLI/MCP database actions | Rust Agent, owner-only CLI attachment, and read-only MCP exist behind modern contracts | Map the original AI workspace and make MySQL read/write tools pass the same product conformance gates. |

## Delivery Order

1. Complete: native read-only object metadata and every matching original
   metadata route, with Axum/dispatch contracts and a real MySQL 8.4 product
   vertical that proves Java remains dormant.
2. Implemented slice: native unparameterized Console statement execution,
   multi-result handling, writes, transactions, history, cancellation, and
   large-cell retrieval. Bind parameters and remaining exact Community edge-case
   conformance are still required for complete parity.
3. Table data editing plus database/schema/table/view DDL preview and execution.
4. Import/export/tasks, datasource lifecycle/SSH/import, account administration,
   routines, structure comparison, pins, and ER metadata.
5. Original AI mapping and MySQL conformance for Agent, CLI, and MCP.

Each stage requires focused unit tests, a real MySQL product vertical with Java
dormancy assertions for native operations, original Web and Tauri contract tests,
the complete repository verification gate, and all GitHub Actions jobs.

## Source Anchors

- `third_party/chat2db-community/chat2db-community-client/src/service/connection.ts`
- `third_party/chat2db-community/chat2db-community-client/src/service/sql.ts`
- `third_party/chat2db-community/chat2db-community-client/src/service/executeSql.ts`
- `third_party/chat2db-community/chat2db-community-client/src/service/importExport.ts`
- `third_party/chat2db-community/chat2db-community-client/src/service/accountAdmin.ts`
- `third_party/chat2db-community/chat2db-community-client/src/service/schemaSync.ts`
- `third_party/chat2db-community/chat2db-community-server/chat2db-community-web/src/main/java/ai/chat2db/community/web/api/controller/`
- `third_party/chat2db-community/chat2db-community-server/chat2db-community-plugins/chat2db-community-mysql/src/main/java/ai/chat2db/plugin/mysql/`
- `crates/chat2db-core/src/native_mysql.rs`
- `crates/chat2db-core/src/large_value.rs`
- `crates/chat2db-core/tests/native_mysql_console_docker.rs`
- `crates/chat2db-storage/src/operation_log.rs`
- `crates/chat2db-storage/migrations/004_operation_log.sql`
- `crates/chat2db-core/tests/native_mysql_product.rs`
- `crates/chat2db-core/src/community.rs`
- `apps/chat2db-web/src/legacy.rs`

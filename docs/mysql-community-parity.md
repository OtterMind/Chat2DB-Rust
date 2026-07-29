# MySQL Community Parity Contract

## Status

- Community baseline: `OtterMind/Chat2DB` `main@3cb8af54cad5bd5caa20bb25f10d9b0e4f01931c`.
- Rust baseline: `OtterMind/Chat2DB-Rust` `main@352838b7d20fad568fd68f5d825e65c56104bd29`.
- Product target: the original Community React frontend running against the Rust Web or Tauri host.
- Runtime-tested now: datasource CRUD/test, database/schema/table discovery, bounded table preview, saved Console CRUD, and one unparameterized read-only MySQL `SELECT` with paging and cancellation.
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
| Table inventory and detail | `/api/rdb/table/list`, `/table_list`, `/table_meta`, `/column_list`, `/index_list`, `/key_list`, `/query` | Table list and preview implemented; modern column/index/key contracts exist behind Java | Route every original endpoint and make MySQL metadata native before Java lease acquisition. |
| Table data operations | `/api/rdb/dml/execute_table`, `/execute_update`, `/get_update_sql`, `/copy_update_sql`, `/copy_in_values_sql`, `/count` | Read-only preview only; closed typed DML generation exists behind modern APIs | Support editable rows, insert/update/delete SQL, counts, copy helpers, optimistic predicates, and bounded execution. |
| Table DDL | `/api/rdb/ddl/*`, `/api/rdb/table/modify/sql`, `/delete`, `/truncate`, `/copy`, create/update examples, DDL export | Partial builder infrastructure only | Match create/alter/drop/truncate/copy preview and execution, including columns, indexes, keys, charset, collation, comments, and MySQL types. |
| Views | `/api/rdb/view/list`, `/column_list`, `/detail`, `/query`, `/view_meta`, `/modify/sql`, `/delete`, `/drop` | Modern view list exists behind Java; no legacy routes | Match list/detail/DDL/data preview and create/alter/drop flows. |
| Functions, procedures, and triggers | `/api/rdb/{function,procedure,trigger}/{list,detail}`, `/api/rdb/routine/{preview_invocation,preview_migration,execute_migration}` | Modern metadata exists behind Java; no legacy routes | Make metadata native, preserve Community body/parameter projection, and map invocation/migration flows. |
| Console SELECT | `/api/rdb/dml/execute`, desktop `sql-execute`/`sql-cancel` | One unparameterized SELECT, paging, limits, cancellation | Add parameters, CTEs, all MySQL read statements, multiple result sets, warnings, affected rows, and Community result shapes. |
| Console scripts and writes | `/api/rdb/dml/execute`, `/execute_ddl` | Not implemented natively | Match Community statement splitting, script execution policy, DDL/DML, transaction settlement, cancellation, and per-statement results. |
| Large cell values | `/api/rdb/cell/value`, `/download`, `/download_path` | Not implemented | Preserve bounded previews and explicit full-value download without loading unbounded cells into the WebView. |
| Saved consoles and SQL history | `/api/operation/saved/*`, `/api/operation/log/{create,list}` and detail | Saved Console CRUD implemented; execution history absent | Persist and filter Community-compatible history and keep restart-safe Console state. |
| SQL parser, formatter, validation, completion | `/api/sql/format`, `/valid_select`, `/api/sql_parser/get_keywords`, `/context/{parser,quick_parser,tip,hover}` | Modern parser/validation/formatter/completion contracts implemented through Java; legacy routes absent | Map every original endpoint to the fixed Community implementation with matching UTF-16 offsets and envelopes. |
| Import, export, and tasks | `/api/import/{sql_file,other_file}`, `/api/export/{sql_file,other_file}`, `/api/task/*`, `/api/rdb/dml/export`, table class generation | Not implemented | Add bounded streaming import/export, progress, stop, download, cleanup, and failure recovery. |
| Account administration | `/api/rdb/account/{capability,list,grants,preview,execute}` | Not implemented | Match MySQL users, hosts, authentication, privileges, role/grant previews, execution, and current Community escaping rules. |
| Structure comparison | `/api/diff/sql` | Not implemented | Match Community structure projection and MySQL synchronization SQL without changing shared query parsing behavior. |
| Pins and ER metadata | `/api/pin/table/*`, `/api/er/*` | Not implemented | Persist pinned tables and expose the metadata needed by the existing ER view. |
| AI, CLI, and MCP | Original `/api/ai` UI plus Community CLI/MCP database actions | Rust Agent, owner-only CLI attachment, and read-only MCP exist behind modern contracts | Map the original AI workspace and make MySQL read/write tools pass the same product conformance gates. |

## Delivery Order

1. Native read-only object metadata and every matching original metadata route.
2. Full Console statement execution, multi-result handling, writes, transactions,
   history, cancellation, and large-cell retrieval.
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
- `crates/chat2db-core/src/community.rs`
- `apps/chat2db-web/src/legacy.rs`

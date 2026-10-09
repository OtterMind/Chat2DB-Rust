//! `MySQL` active-transaction inspection for the Community operations monitor.

use std::time::Duration;

use chat2db_contract::ActiveTransaction;
use mysql_async::{Error as MysqlError, prelude::Queryable};

use crate::{
    AppError, Application,
    native_mysql::{finish_connection, open_resolved_connection, resolve_native_connection},
};

const ACTIVE_TRANSACTION_TIMEOUT: Duration = Duration::from_secs(30);
const ACTIVE_TRANSACTION_UNAVAILABLE: &str = "mysql.activeTransaction.unavailable";
const ACTIVE_TRANSACTION_PRIVILEGE: &str = "mysql.activeTransaction.processPrivilegeRequired";

const SELECT_ACTIVE_TRANSACTIONS: &str = "
    SELECT t.trx_id,
           t.trx_state,
           TIMESTAMPDIFF(MICROSECOND, t.trx_started, NOW(6)) DIV 1000,
           t.trx_isolation_level,
           t.trx_rows_locked,
           t.trx_rows_modified,
           t.trx_lock_structs,
           t.trx_mysql_thread_id,
           p.USER,
           p.HOST,
           p.DB,
           COALESCE(p.INFO, '')
    FROM information_schema.INNODB_TRX t
    LEFT JOIN information_schema.PROCESSLIST p ON p.ID = t.trx_mysql_thread_id
    ORDER BY t.trx_started, t.trx_id";

type ActiveTransactionRow = (
    String,
    String,
    Option<i64>,
    String,
    u64,
    u64,
    u64,
    u64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

impl Application {
    /// Lists active `InnoDB` transactions of one native `MySQL` datasource.
    ///
    /// # Errors
    ///
    /// Returns connection, privilege, timeout, or query failures. A missing
    /// `PROCESS` privilege answers `mysql.activeTransaction.processPrivilegeRequired`
    /// so the monitor can explain the requirement.
    pub async fn list_mysql_active_transactions(
        &self,
        datasource_id: &str,
    ) -> Result<Vec<ActiveTransaction>, AppError> {
        let resolved = resolve_native_connection(self, datasource_id).await?;
        let mut connection = open_resolved_connection(&resolved).await?;
        let rows = match tokio::time::timeout(
            ACTIVE_TRANSACTION_TIMEOUT,
            connection.query::<ActiveTransactionRow, _>(SELECT_ACTIVE_TRANSACTIONS),
        )
        .await
        {
            Ok(Ok(rows)) => rows,
            Ok(Err(error)) => {
                return finish_connection(connection, Err(active_transaction_failure(&error)))
                    .await;
            }
            Err(_) => {
                return finish_connection(
                    connection,
                    Err(AppError::invalid(
                        ACTIVE_TRANSACTION_UNAVAILABLE,
                        "The MySQL active transaction query timed out",
                    )),
                )
                .await;
            }
        };
        finish_connection(connection, Ok(())).await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    trx_id,
                    state,
                    age_millis,
                    isolation_level,
                    rows_locked,
                    rows_modified,
                    lock_structs,
                    thread_id,
                    user,
                    host,
                    database,
                    query,
                )| {
                    let age_millis = age_millis.unwrap_or_default();
                    ActiveTransaction {
                        trx_id,
                        state,
                        started_at_ms: current_epoch_millis()
                            .saturating_sub(age_millis)
                            .to_string(),
                        age_seconds: u64::try_from(age_millis / 1_000).unwrap_or_default(),
                        isolation_level,
                        rows_locked,
                        rows_modified,
                        lock_structs,
                        thread_id,
                        user,
                        host,
                        database,
                        query: query.unwrap_or_default(),
                    }
                },
            )
            .collect())
    }
}

fn current_epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn active_transaction_failure(error: &MysqlError) -> AppError {
    match error {
        MysqlError::Server(server) if server.code == 1227 || server.code == 1142 => {
            AppError::invalid(
                ACTIVE_TRANSACTION_PRIVILEGE,
                "Inspecting active transactions requires the PROCESS privilege",
            )
        }
        MysqlError::Server(server) => AppError::invalid(
            ACTIVE_TRANSACTION_UNAVAILABLE,
            format!(
                "The MySQL active transaction query failed: {}",
                server.message
            ),
        ),
        _ => AppError::invalid(
            ACTIVE_TRANSACTION_UNAVAILABLE,
            "The MySQL connection ended before the active transaction query completed",
        ),
    }
}

use chat2db_contract::{
    CommunityErModel, CommunityErPositionRequest, CommunityErQueryRequest,
    CommunityPinnedTableList, CommunityPinnedTableRequest,
};

use crate::{AppError, Application, storage_call};

impl Application {
    /// Pins one `MySQL` table in the local workspace.
    ///
    /// # Errors
    ///
    /// Returns datasource, validation, availability, or storage failures.
    pub async fn pin_community_mysql_table(
        &self,
        request: CommunityPinnedTableRequest,
    ) -> Result<(), AppError> {
        self.get_datasource(&request.data_source_id).await?;
        let storage = self.require_storage()?;
        storage_call(move || {
            storage.pin_mysql_table(
                &request.data_source_id,
                &request.database_name,
                &request.schema_name,
                &request.table_name,
            )
        })
        .await
    }

    /// Removes one pinned `MySQL` table from the local workspace.
    ///
    /// # Errors
    ///
    /// Returns datasource, validation, availability, or storage failures.
    pub async fn unpin_community_mysql_table(
        &self,
        request: CommunityPinnedTableRequest,
    ) -> Result<(), AppError> {
        self.get_datasource(&request.data_source_id).await?;
        let storage = self.require_storage()?;
        storage_call(move || {
            storage.unpin_mysql_table(
                &request.data_source_id,
                &request.database_name,
                &request.schema_name,
                &request.table_name,
            )
        })
        .await
    }

    /// Lists pinned `MySQL` table names for one database/schema scope.
    ///
    /// # Errors
    ///
    /// Returns datasource, validation, availability, or storage failures.
    pub async fn list_community_mysql_pinned_tables(
        &self,
        request: CommunityPinnedTableRequest,
    ) -> Result<CommunityPinnedTableList, AppError> {
        self.get_datasource(&request.data_source_id).await?;
        let storage = self.require_storage()?;
        storage_call(move || {
            storage
                .list_mysql_pinned_tables(
                    &request.data_source_id,
                    &request.database_name,
                    &request.schema_name,
                )
                .map(|items| CommunityPinnedTableList { items })
        })
        .await
    }

    /// Loads native `MySQL` ER metadata and the last persisted canvas layout.
    ///
    /// # Errors
    ///
    /// Returns datasource, `MySQL` metadata, validation, availability, or storage failures.
    pub async fn community_mysql_er_model(
        &self,
        request: CommunityErQueryRequest,
    ) -> Result<CommunityErModel, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.data_source_id)
            .await?;
        let table_driver = driver.tables().ok_or_else(|| {
            AppError::invalid(
                "native_table_capability_not_available",
                "The native Rust driver does not implement table operations",
            )
        })?;
        let tables = table_driver
            .load_er_tables(
                self,
                &request.data_source_id,
                &request.database_name,
                &request.schema_name,
            )
            .await?;
        let storage = self.require_storage()?;
        let position = storage_call(move || {
            storage.mysql_er_position(
                &request.data_source_id,
                &request.database_name,
                &request.schema_name,
            )
        })
        .await?;
        Ok(CommunityErModel { tables, position })
    }

    /// Persists the Community ER canvas layout using a true upsert.
    ///
    /// # Errors
    ///
    /// Returns datasource, validation, availability, or storage failures.
    pub async fn save_community_mysql_er_position(
        &self,
        request: CommunityErPositionRequest,
    ) -> Result<(), AppError> {
        self.get_datasource(&request.data_source_id).await?;
        let storage = self.require_storage()?;
        storage_call(move || {
            storage.save_mysql_er_position(
                &request.data_source_id,
                &request.database_name,
                &request.schema_name,
                &request.position,
            )
        })
        .await
    }
}

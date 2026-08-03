use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Datasource;

/// Community datasource file formats accepted by the compatibility importer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommunityDatasourceImportFormat {
    /// `Chat2DB`'s portable object or legacy datasource array JSON.
    Chat2dbJson,
    /// Navicat connection export XML (`.ncx`).
    NavicatNcx,
    /// `DBeaver` project export ZIP (`.dbp`).
    DbeaverDbp,
    /// `DataGrip` clipboard datasource settings text.
    DatagripText,
}

/// Raw Community datasource file submitted to Core for validation and import.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatasourceFileImportRequest {
    /// Explicit format selected by the delivery adapter.
    pub format: CommunityDatasourceImportFormat,
    /// Raw file bytes. Core applies its own size limits before parsing.
    pub content: Vec<u8>,
}

impl Debug for CommunityDatasourceFileImportRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommunityDatasourceFileImportRequest")
            .field("format", &self.format)
            .field(
                "content",
                &format_args!("[REDACTED; {} bytes]", self.content.len()),
            )
            .finish()
    }
}

/// Result of importing one third-party or legacy datasource file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatasourceFileImportResult {
    /// Number of new datasource records created.
    pub count: u32,
    /// Secret-free metadata for the new records.
    pub created: Vec<Datasource>,
    /// Entries intentionally ignored because they are not supported native `MySQL` definitions.
    pub skipped: u32,
}

#[cfg(test)]
mod tests {
    use super::{CommunityDatasourceFileImportRequest, CommunityDatasourceImportFormat};

    #[test]
    fn import_request_debug_never_exposes_file_contents() {
        let request = CommunityDatasourceFileImportRequest {
            format: CommunityDatasourceImportFormat::DbeaverDbp,
            content: b"sentinel-password-and-token".to_vec(),
        };

        let debug = format!("{request:?}");
        assert!(!debug.contains("sentinel-password"));
        assert!(debug.contains("27 bytes"));
    }
}

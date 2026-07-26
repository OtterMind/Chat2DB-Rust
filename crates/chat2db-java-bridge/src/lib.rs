//! Supervised access to the private Java database compatibility engine.

mod command;
mod error;
mod state;
mod stderr_tail;
mod supervisor;

pub use command::EngineCommand;
pub use error::{
    BridgeError, DatabaseErrorCause, DatabaseErrorDetail, DeliveryOutcome, RemoteEngineError,
};
pub use state::{
    EngineIdentity, EngineState, PingReply, ProcessExit, SessionState, StderrSnapshot,
};
pub use supervisor::{
    COMMUNITY_OBJECT_METADATA_CAPABILITY, COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
    COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY, COMMUNITY_RELATION_METADATA_CAPABILITY,
    COMMUNITY_SCHEMA_METADATA_CAPABILITY, COMMUNITY_SQL_BUILDER_CAPABILITY,
    COMMUNITY_SQL_PARSER_CAPABILITY, CancelDisposition, ColumnNullability, CommunityClasspath,
    CommunityClient, CommunityDatabase, CommunityDriverConfig, CommunityForeignKey,
    CommunityFunction, CommunityFunctionParameter, CommunityParsedStatement, CommunityPlugin,
    CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices, CommunityPrimaryKey,
    CommunityProcedure, CommunityProcedureParameter, CommunitySchema, CommunitySqlAnalysis,
    CommunityTable, CommunityTableColumn, CommunityTableIndex, CommunityTableIndexColumn,
    CommunityTrigger, ConnectionProperty, DRIVER_EXTERNAL_JAR_CAPABILITY, DatabaseProduct,
    DriverArtifact, DriverClient, DriverSpec, EngineClient, EngineConfig, EngineSupervisor,
    FLOW_CREDIT_CAPABILITY, JdbcColumn, JdbcParameter, JdbcRow, JdbcValue, JdbcValueType,
    LoadedDriver, MAX_DRIVER_ARTIFACT_BYTES, MAX_DRIVER_ARTIFACTS, MAX_DRIVER_TOTAL_BYTES,
    OPERATION_CANCEL_CAPABILITY, QUERY_TYPED_BATCHES_CAPABILITY, QueryCompleted, QueryEvent,
    QueryOptions, QueryRequest, QueryStarted, QueryStream, RowBatch, SESSION_JDBC_CAPABILITY,
    Session, SessionConfig, TRANSACTION_LOCAL_CAPABILITY, Transaction, TransactionIsolation,
    TransactionOptions, UPDATE_JDBC_CAPABILITY, UpdateRequest, UpdateResult,
};

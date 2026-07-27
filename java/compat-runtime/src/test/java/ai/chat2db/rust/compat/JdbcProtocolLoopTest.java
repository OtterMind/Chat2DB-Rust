package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.BeginTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.BuildCommunityDmlRequest;
import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.CancelOperationRequest;
import ai.chat2db.rust.compat.protocol.v1.ClientEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ClientHello;
import ai.chat2db.rust.compat.protocol.v1.CloseSessionRequest;
import ai.chat2db.rust.compat.protocol.v1.ConnectionProperty;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlColumn;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlRow;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlSingleInsert;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTarget;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlValue;
import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.ExecuteQueryRequest;
import ai.chat2db.rust.compat.protocol.v1.ExecuteUpdateRequest;
import ai.chat2db.rust.compat.protocol.v1.ErrorCategory;
import ai.chat2db.rust.compat.protocol.v1.FormatCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.GrantCreditsRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityFunctionRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityProcedureRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityTriggerRequest;
import ai.chat2db.rust.compat.protocol.v1.JdbcParameter;
import ai.chat2db.rust.compat.protocol.v1.JdbcValue;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityColumnsRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityDatabasesRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityFunctionsRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityIndexesRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityProceduresRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunitySchemasRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityTableKeysRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityTablesRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityTriggersRequest;
import ai.chat2db.rust.compat.protocol.v1.ListCommunityViewsRequest;
import ai.chat2db.rust.compat.protocol.v1.OpenSessionRequest;
import ai.chat2db.rust.compat.protocol.v1.ProtocolVersion;
import ai.chat2db.rust.compat.protocol.v1.QueryOptions;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.RollbackTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import ai.chat2db.rust.compat.protocol.v1.Shutdown;
import ai.chat2db.rust.compat.protocol.v1.TransactionIsolation;
import ai.chat2db.rust.compat.protocol.v1.UnloadDriverRequest;
import ai.chat2db.rust.compat.protocol.v1.ValidateCommunitySqlRequest;
import com.google.protobuf.ByteString;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PipedInputStream;
import java.io.PipedOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.junit.jupiter.api.Test;

class JdbcProtocolLoopTest {

    private static final Duration TIMEOUT = Duration.ofSeconds(10);
    private static final String SQL_SENTINEL = "sql-do-not-log";
    private static final String CELL_SENTINEL = "cell-do-not-log";
    private static final String PASSWORD_SENTINEL = "password-do-not-log";

    @Test
    void communityDmlDispatchDoesNotRequireAJdbcSession() throws Exception {
        String valueSentinel = "community-dml-value-do-not-log";
        BuildCommunityDmlRequest request = BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(CommunityDmlTarget.newBuilder()
                        .setSchemaName("APP")
                        .setTableName("items"))
                .setSingleInsert(CommunityDmlSingleInsert.newBuilder()
                        .addColumns(CommunityDmlColumn.newBuilder()
                                .setName("label")
                                .setDataTypeName("VARCHAR"))
                        .setRow(CommunityDmlRow.newBuilder()
                                .addValues(CommunityDmlValue.newBuilder()
                                        .setStringValue(valueSentinel))))
                .build();

        try (Harness harness = new Harness()) {
            harness.send(hello());
            assertEquals(ServerEnvelope.PayloadCase.HELLO, harness.read().getPayloadCase());
            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("community-dml"))
                    .setBuildCommunityDml(request)
                    .build());

            ServerEnvelope response = harness.read();
            if (isCommunityCompatibilityConfigured()) {
                assertEquals(
                        ServerEnvelope.PayloadCase.COMMUNITY_BUILT_DML,
                        response.getPayloadCase());
                assertEquals(
                        "INSERT INTO APP.items (label)  VALUES "
                                + "('community-dml-value-do-not-log')",
                        response.getCommunityBuiltDml().getSql());
            } else {
                assertEquals(ServerEnvelope.PayloadCase.ERROR, response.getPayloadCase());
                assertEquals("community.plugin_not_found", response.getError().getCode());
                assertFalse(response.getError().getCode().equals("session.id_required"));
                assertFalse(response.getError().getMessage().contains(valueSentinel));
            }
            assertFalse(harness.diagnostics().contains(valueSentinel));
        }
    }

    @Test
    void loadsExternalH2AndStreamsTypedRowsWithTransactionsAndBackpressure()
            throws Exception {
        Path h2Jar = h2DriverJar();
        byte[] digest = sha256(h2Jar);

        try (Harness harness = new Harness()) {
            harness.send(hello());
            ServerEnvelope hello = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.HELLO, hello.getPayloadCase());
            assertEquals(
                    ProtocolLoop.capabilities(isCommunityCompatibilityConfigured()),
                    hello.getHello().getCapabilitiesList());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("load"))
                    .setLoadDriver(LoadDriverRequest.newBuilder()
                            .setDriverClass("org.h2.Driver")
                            .addArtifacts(DriverArtifact.newBuilder()
                                    .setPath(h2Jar.toString())
                                    .setSha256(ByteString.copyFrom(digest))))
                    .build());
            ServerEnvelope loaded = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.DRIVER_LOADED, loaded.getPayloadCase());
            assertEquals(expectedDriverId("org.h2.Driver", digest), loaded.getDriverLoaded().getDriverId());
            String driverId = loaded.getDriverLoaded().getDriverId();

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("open"))
                    .setOpenSession(OpenSessionRequest.newBuilder()
                            .setDriverId(driverId)
                            .setJdbcUrl("jdbc:h2:mem:stage3;DB_CLOSE_DELAY=-1")
                            .addProperties(ConnectionProperty.newBuilder()
                                    .setKey("user")
                                    .setValue("sa"))
                            .addProperties(ConnectionProperty.newBuilder()
                                    .setKey("password")
                                    .setValue(PASSWORD_SENTINEL)
                                    .setSensitive(true)))
                    .build());
            ServerEnvelope opened = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.SESSION_OPENED, opened.getPayloadCase());
            assertEquals(SessionState.SESSION_STATE_AUTO_COMMIT, opened.getSessionOpened().getSessionState());
            String sessionId = opened.getSessionOpened().getSessionId();

            assertUpdate(
                    harness,
                    sessionId,
                    "create",
                    "CREATE TABLE items(id BIGINT PRIMARY KEY, secret VARCHAR)",
                    0);
            assertUpdate(
                    harness,
                    sessionId,
                    "insert",
                    "INSERT INTO items VALUES (7, ?)",
                    1,
                    JdbcParameter.newBuilder()
                            .setPosition(1)
                            .setValue(JdbcValue.newBuilder().setTextValue(CELL_SENTINEL))
                            .build());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("query", sessionId))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT id, secret FROM items /* " + SQL_SENTINEL + " */")
                            .setOptions(QueryOptions.newBuilder()
                                    .setTargetBatchRows(1)
                                    .setTargetBatchBytes(1024)))
                    .build());
            ServerEnvelope started = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.QUERY_STARTED, started.getPayloadCase());
            assertEquals(0, started.getMeta().getSequence());
            assertFalse(started.getMeta().getTerminal());
            assertEquals(0, harness.available(), "row batches must pause at zero credit");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("credit", sessionId).toBuilder()
                            .setTraceId("t".repeat(ProtocolLimits.MAX_DRIVER_ID_BYTES)))
                    .setGrantCredits(GrantCreditsRequest.newBuilder()
                            .setTargetRequestId("query")
                            .setBatchCredits(1))
                    .build());
            List<ServerEnvelope> queryResponses = harness.read(3);
            ServerEnvelope credit = byRequest(queryResponses, "credit").get(0);
            assertEquals(ServerEnvelope.PayloadCase.CREDITS_GRANTED, credit.getPayloadCase());
            assertEquals(1, credit.getCreditsGranted().getAcceptedBatchCredits());
            assertTrue(credit.getSerializedSize() <= 1024);
            List<ServerEnvelope> stream = byRequest(queryResponses, "query").stream()
                    .sorted(Comparator.comparingLong(response -> response.getMeta().getSequence()))
                    .toList();
            assertEquals(List.of(1L, 2L), stream.stream()
                    .map(response -> response.getMeta().getSequence())
                    .toList());
            assertEquals(ServerEnvelope.PayloadCase.ROW_BATCH, stream.get(0).getPayloadCase());
            assertEquals(7, stream.get(0)
                    .getRowBatch()
                    .getRows(0)
                    .getValues(0)
                    .getSignedIntegerValue());
            assertEquals(CELL_SENTINEL, stream.get(0)
                    .getRowBatch()
                    .getRows(0)
                    .getValues(1)
                    .getTextValue());
            assertEquals(ServerEnvelope.PayloadCase.QUERY_COMPLETED, stream.get(1).getPayloadCase());
            assertTrue(stream.get(1).getMeta().getTerminal());

            long exactResultBytes = stream.get(0).getRowBatch().getRows(0).getSerializedSize();
            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("exact-byte-cap", sessionId))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT id, secret FROM items")
                            .setOptions(QueryOptions.newBuilder()
                                    .setInitialBatchCredits(1)
                                    .setTargetBatchRows(2)
                                    .setTargetBatchBytes(1024)
                                    .setMaxResultBytes(exactResultBytes)))
                    .build());
            List<ServerEnvelope> exactResponses = harness.read(3);
            ServerEnvelope exactCompleted = exactResponses.get(2);
            assertEquals(ServerEnvelope.PayloadCase.QUERY_COMPLETED, exactCompleted.getPayloadCase());
            assertEquals(1, exactCompleted.getQueryCompleted().getRowCount());
            assertFalse(exactCompleted.getQueryCompleted().getTruncatedByMaxResultBytes());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-database", sessionId))
                    .setListCommunitySchemas(ListCommunitySchemasRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1)))
                    .build());
            ServerEnvelope oversizedCommunityDatabase = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, oversizedCommunityDatabase.getPayloadCase());
            assertEquals("protocol.limit_exceeded", oversizedCommunityDatabase.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    oversizedCommunityDatabase.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-database-type", sessionId))
                    .setListCommunityDatabases(ListCommunityDatabasesRequest.newBuilder()
                            .setDatabaseType("  "))
                    .build());
            ServerEnvelope blankCommunityDatabaseType = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, blankCommunityDatabaseType.getPayloadCase());
            assertEquals(
                    "protocol.invalid_database_type",
                    blankCommunityDatabaseType.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    blankCommunityDatabaseType.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-table-pattern", sessionId))
                    .setListCommunityTables(ListCommunityTablesRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setTableNamePattern("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1)))
                    .build());
            ServerEnvelope oversizedCommunityTablePattern = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    oversizedCommunityTablePattern.getPayloadCase());
            assertEquals(
                    "protocol.limit_exceeded",
                    oversizedCommunityTablePattern.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    oversizedCommunityTablePattern.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-column-table", sessionId))
                    .setListCommunityColumns(ListCommunityColumnsRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setTableName("  "))
                    .build());
            ServerEnvelope blankCommunityColumnTable = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, blankCommunityColumnTable.getPayloadCase());
            assertEquals(
                    "protocol.invalid_table_name",
                    blankCommunityColumnTable.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    blankCommunityColumnTable.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-index-schema", sessionId))
                    .setListCommunityIndexes(ListCommunityIndexesRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSchemaName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1))
                            .setTableName("items"))
                    .build());
            ServerEnvelope oversizedCommunityIndexSchema = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    oversizedCommunityIndexSchema.getPayloadCase());
            assertEquals(
                    "protocol.limit_exceeded",
                    oversizedCommunityIndexSchema.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    oversizedCommunityIndexSchema.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("missing-community-object-plugin", sessionId))
                    .setListCommunityIndexes(ListCommunityIndexesRequest.newBuilder()
                            .setDatabaseType("MISSING")
                            .setTableName("items"))
                    .build());
            ServerEnvelope missingCommunityObjectPlugin = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    missingCommunityObjectPlugin.getPayloadCase());
            assertEquals(
                    "community.plugin_not_found",
                    missingCommunityObjectPlugin.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    missingCommunityObjectPlugin.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-view-pattern", sessionId))
                    .setListCommunityViews(ListCommunityViewsRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setViewNamePattern("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1)))
                    .build());
            ServerEnvelope oversizedCommunityViewPattern = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    oversizedCommunityViewPattern.getPayloadCase());
            assertEquals(
                    "protocol.limit_exceeded",
                    oversizedCommunityViewPattern.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    oversizedCommunityViewPattern.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-imported-key-table", sessionId))
                    .setListCommunityImportedKeys(ListCommunityTableKeysRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setTableName("  "))
                    .build());
            ServerEnvelope blankCommunityImportedKeyTable = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    blankCommunityImportedKeyTable.getPayloadCase());
            assertEquals(
                    "protocol.invalid_table_name",
                    blankCommunityImportedKeyTable.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    blankCommunityImportedKeyTable.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-exported-key-schema", sessionId))
                    .setListCommunityExportedKeys(ListCommunityTableKeysRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSchemaName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1))
                            .setTableName("items"))
                    .build());
            ServerEnvelope oversizedCommunityExportedKeySchema = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    oversizedCommunityExportedKeySchema.getPayloadCase());
            assertEquals(
                    "protocol.limit_exceeded",
                    oversizedCommunityExportedKeySchema.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    oversizedCommunityExportedKeySchema.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-primary-key-database-type", sessionId))
                    .setListCommunityPrimaryKeys(ListCommunityTableKeysRequest.newBuilder()
                            .setDatabaseType("  ")
                            .setTableName("items"))
                    .build());
            ServerEnvelope blankCommunityPrimaryKeyDatabaseType = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    blankCommunityPrimaryKeyDatabaseType.getPayloadCase());
            assertEquals(
                    "protocol.invalid_database_type",
                    blankCommunityPrimaryKeyDatabaseType.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    blankCommunityPrimaryKeyDatabaseType.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-function-database", sessionId))
                    .setListCommunityFunctions(ListCommunityFunctionsRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName("  "))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_database_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-function-name", sessionId))
                    .setGetCommunityFunction(GetCommunityFunctionRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setFunctionName("  "))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_function_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-function-parameter-schema", sessionId))
                    .setListCommunityFunctionParameters(GetCommunityFunctionRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSchemaName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1))
                            .setFunctionName("calculate"))
                    .build());
            assertCommunityValidationFailure(harness, "protocol.limit_exceeded");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-procedure-database", sessionId))
                    .setListCommunityProcedures(ListCommunityProceduresRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName(""))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_database_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-procedure-name", sessionId))
                    .setGetCommunityProcedure(GetCommunityProcedureRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setProcedureName("  "))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_procedure_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-procedure-parameter-database", sessionId))
                    .setListCommunityProcedureParameters(GetCommunityProcedureRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1))
                            .setProcedureName("record_event"))
                    .build());
            assertCommunityValidationFailure(harness, "protocol.limit_exceeded");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-trigger-database", sessionId))
                    .setListCommunityTriggers(ListCommunityTriggersRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName("  "))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_database_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-trigger-name", sessionId))
                    .setGetCommunityTrigger(GetCommunityTriggerRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setTriggerName(""))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_trigger_name");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-format-sql", sessionId))
                    .setFormatCommunitySql(FormatCommunitySqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSql("x".repeat(ProtocolLimits.MAX_SQL_BYTES + 1)))
                    .build());
            assertCommunityValidationFailure(harness, "protocol.limit_exceeded");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-format-type", sessionId))
                    .setFormatCommunitySql(FormatCommunitySqlRequest.newBuilder()
                            .setDatabaseType("  ")
                            .setSql("SELECT 1"))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_database_type");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("format-community-h2", sessionId))
                    .setFormatCommunitySql(FormatCommunitySqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSql("select id,secret from items where id=7"))
                    .build());
            ServerEnvelope formattedCommunitySql = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.COMMUNITY_FORMATTED_SQL,
                    formattedCommunitySql.getPayloadCase());
            assertTrue(formattedCommunitySql.getCommunityFormattedSql().getSql().contains("\n"));
            assertTrue(formattedCommunitySql
                    .getCommunityFormattedSql()
                    .getSql()
                    .contains("from\n  items"));

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-validation-sql", sessionId))
                    .setValidateCommunitySql(ValidateCommunitySqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setSql("x".repeat(ProtocolLimits.MAX_SQL_BYTES + 1)))
                    .build());
            assertCommunityValidationFailure(harness, "protocol.limit_exceeded");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("blank-community-validation-type", sessionId))
                    .setValidateCommunitySql(ValidateCommunitySqlRequest.newBuilder()
                            .setDatabaseType("  ")
                            .setSql("SELECT 1"))
                    .build());
            assertCommunityValidationFailure(
                    harness, "protocol.invalid_database_type");

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("missing-community-validation-plugin", sessionId))
                    .setValidateCommunitySql(ValidateCommunitySqlRequest.newBuilder()
                            .setDatabaseType("MISSING")
                            .setSql("SELECT 1"))
                    .build());
            assertCommunityValidationFailure(harness, "community.plugin_not_found");

            if (isCommunityCompatibilityConfigured()) {
                harness.send(ClientEnvelope.newBuilder()
                        .setMeta(meta("valid-community-sql", sessionId))
                        .setValidateCommunitySql(ValidateCommunitySqlRequest.newBuilder()
                                .setDatabaseType("H2")
                                .setSql("SELECT 1;"))
                        .build());
                ServerEnvelope validCommunitySql = harness.read();
                assertEquals(
                        ServerEnvelope.PayloadCase.COMMUNITY_SQL_VALIDATION,
                        validCommunitySql.getPayloadCase());
                assertTrue(validCommunitySql.getCommunitySqlValidation().getValid());
                assertFalse(validCommunitySql
                        .getCommunitySqlValidation()
                        .getStatementsList()
                        .isEmpty());

                harness.send(ClientEnvelope.newBuilder()
                        .setMeta(meta("invalid-community-sql", sessionId))
                        .setValidateCommunitySql(ValidateCommunitySqlRequest.newBuilder()
                                .setDatabaseType("H2")
                                .setSql("SELECT FROM;"))
                        .build());
                ServerEnvelope invalidCommunitySql = harness.read();
                assertEquals(
                        ServerEnvelope.PayloadCase.COMMUNITY_SQL_VALIDATION,
                        invalidCommunitySql.getPayloadCase());
                assertFalse(invalidCommunitySql.getCommunitySqlValidation().getValid());
                assertFalse(invalidCommunitySql
                        .getCommunitySqlValidation()
                        .getDiagnosticsList()
                        .isEmpty());
            }

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("begin", sessionId))
                    .setBeginTransaction(BeginTransactionRequest.newBuilder()
                            .setIsolation(TransactionIsolation.TRANSACTION_ISOLATION_READ_COMMITTED))
                    .build());
            ServerEnvelope transaction = harness.read();
            assertEquals("begin", transaction.getMeta().getRequestId());
            assertEquals(
                    ServerEnvelope.PayloadCase.TRANSACTION_STARTED,
                    transaction.getPayloadCase());
            assertEquals(
                    SessionState.SESSION_STATE_TRANSACTION_ACTIVE,
                    transaction.getTransactionStarted().getSessionState());
            String transactionId = transaction.getTransactionStarted().getTransactionId();

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("oversized-community-database-in-transaction", sessionId))
                    .setListCommunitySchemas(ListCommunitySchemasRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setDatabaseName("x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1))
                            .setTransactionId(transactionId))
                    .build());
            ServerEnvelope oversizedCommunityDatabaseInTransaction = harness.read();
            assertEquals(
                    ServerEnvelope.PayloadCase.ERROR,
                    oversizedCommunityDatabaseInTransaction.getPayloadCase());
            assertEquals(
                    "protocol.limit_exceeded",
                    oversizedCommunityDatabaseInTransaction.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_TRANSACTION_ACTIVE,
                    oversizedCommunityDatabaseInTransaction.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("missing-community-plugin", sessionId))
                    .setListCommunitySchemas(ListCommunitySchemasRequest.newBuilder()
                            .setDatabaseType("MISSING")
                            .setTransactionId(transactionId))
                    .build());
            ServerEnvelope missingCommunityPlugin = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, missingCommunityPlugin.getPayloadCase());
            assertEquals("community.plugin_not_found", missingCommunityPlugin.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_TRANSACTION_ACTIVE,
                    missingCommunityPlugin.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("tx-insert", sessionId))
                    .setExecuteUpdate(ExecuteUpdateRequest.newBuilder()
                            .setSql("INSERT INTO items VALUES (7, 'duplicate')")
                            .setTransactionId(transactionId))
                    .build());
            ServerEnvelope failedUpdate = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, failedUpdate.getPayloadCase());
            assertEquals(
                    SessionState.SESSION_STATE_ROLLBACK_REQUIRED,
                    failedUpdate.getError().getSessionState());
            assertEquals(
                    ai.chat2db.rust.compat.protocol.v1.OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                    failedUpdate.getError().getOutcome());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("rollback", sessionId))
                    .setRollbackTransaction(RollbackTransactionRequest.newBuilder()
                            .setTransactionId(transactionId))
                    .build());
            ServerEnvelope rolledBack = harness.read();
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    rolledBack.getTransactionRolledBack().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("byte-cap", sessionId))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT secret FROM items")
                            .setOptions(QueryOptions.newBuilder()
                                    .setInitialBatchCredits(1)
                                    .setTargetBatchRows(1)
                                    .setTargetBatchBytes(1024)
                                    .setMaxResultBytes(1)))
                    .build());
            ServerEnvelope cappedStarted = harness.read();
            ServerEnvelope cappedCompleted = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.QUERY_STARTED, cappedStarted.getPayloadCase());
            assertEquals(ServerEnvelope.PayloadCase.QUERY_COMPLETED, cappedCompleted.getPayloadCase());
            assertEquals(0, cappedCompleted.getQueryCompleted().getRowCount());
            assertTrue(cappedCompleted.getQueryCompleted().getTruncatedByMaxResultBytes());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("invalid-byte-cap", sessionId))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT 1")
                            .setOptions(QueryOptions.newBuilder()
                                    .setMaxResultBytes(ProtocolLimits.MAX_RESULT_BYTES + 1)))
                    .build());
            ServerEnvelope invalidByteCap = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, invalidByteCap.getPayloadCase());
            assertEquals("query.invalid_max_result_bytes", invalidByteCap.getError().getCode());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    invalidByteCap.getError().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("cancelled-query", sessionId))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT x FROM SYSTEM_RANGE(1, 1000)")
                            .setOptions(QueryOptions.newBuilder()
                                    .setTargetBatchRows(1)
                                    .setTargetBatchBytes(1024)))
                    .build());
            assertEquals(ServerEnvelope.PayloadCase.QUERY_STARTED, harness.read().getPayloadCase());
            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("cancel", sessionId).toBuilder()
                            .setTraceId("t".repeat(ProtocolLimits.MAX_DRIVER_ID_BYTES)))
                    .setCancelOperation(CancelOperationRequest.newBuilder()
                            .setTargetRequestId("cancelled-query")
                            .setReason("cancel-reason-do-not-log"))
                    .build());
            List<ServerEnvelope> cancellationResponses = harness.read(2);
            ServerEnvelope cancelAck = byRequest(cancellationResponses, "cancel").get(0);
            assertEquals(
                    CancelDisposition.CANCEL_DISPOSITION_ACCEPTED,
                    cancelAck.getOperationCancelled().getDisposition());
            assertTrue(cancelAck.getSerializedSize() <= 1024);
            ServerEnvelope cancelledQuery = byRequest(cancellationResponses, "cancelled-query").get(0);
            assertEquals(ServerEnvelope.PayloadCase.ERROR, cancelledQuery.getPayloadCase());
            assertEquals(ErrorCategory.ERROR_CATEGORY_CANCELLED, cancelledQuery.getError().getCategory());
            assertEquals(
                    SessionState.SESSION_STATE_AUTO_COMMIT,
                    cancelledQuery.getError().getSessionState());
            assertTrue(cancelledQuery.getMeta().getTerminal());
            assertEquals(1, cancelledQuery.getMeta().getSequence());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("deadline-query", sessionId).toBuilder()
                            .setDeadlineUnixMillis(System.currentTimeMillis() + 500))
                    .setExecuteQuery(ExecuteQueryRequest.newBuilder()
                            .setSql("SELECT x FROM SYSTEM_RANGE(1, 1000)")
                            .setOptions(QueryOptions.newBuilder()
                                    .setTargetBatchRows(1)
                                    .setTargetBatchBytes(1024)))
                    .build());
            ServerEnvelope deadlineStarted = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.QUERY_STARTED, deadlineStarted.getPayloadCase());
            ServerEnvelope deadlineFailure = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, deadlineFailure.getPayloadCase());
            assertEquals(ErrorCategory.ERROR_CATEGORY_DEADLINE, deadlineFailure.getError().getCategory());
            assertEquals(1, deadlineFailure.getMeta().getSequence());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("close", sessionId))
                    .setCloseSession(CloseSessionRequest.getDefaultInstance())
                    .build());
            ServerEnvelope closed = harness.read();
            assertEquals(SessionState.SESSION_STATE_CLOSED, closed.getSessionClosed().getSessionState());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("unload"))
                    .setUnloadDriver(UnloadDriverRequest.newBuilder().setDriverId(driverId))
                    .build());
            assertEquals(ServerEnvelope.PayloadCase.DRIVER_UNLOADED, harness.read().getPayloadCase());

            harness.send(ClientEnvelope.newBuilder()
                    .setMeta(meta("shutdown"))
                    .setShutdown(Shutdown.getDefaultInstance())
                    .build());
            assertEquals(ServerEnvelope.PayloadCase.SHUTDOWN_ACK, harness.read().getPayloadCase());
            assertEquals(CompatibilityRuntime.EXIT_OK, harness.awaitExit());

            String diagnostics = harness.diagnostics();
            assertFalse(diagnostics.contains(SQL_SENTINEL));
            assertFalse(diagnostics.contains(CELL_SENTINEL));
            assertFalse(diagnostics.contains(PASSWORD_SENTINEL));
            assertFalse(diagnostics.contains("cancel-reason-do-not-log"));
        }
    }

    private static void assertUpdate(
            Harness harness,
            String sessionId,
            String requestId,
            String sql,
            long affectedRows,
            JdbcParameter... parameters)
            throws Exception {
        harness.send(ClientEnvelope.newBuilder()
                .setMeta(meta(requestId, sessionId).toBuilder()
                        .setTraceId("t".repeat(ProtocolLimits.MAX_DRIVER_ID_BYTES)))
                .setExecuteUpdate(ExecuteUpdateRequest.newBuilder()
                        .setSql(sql)
                        .addAllParameters(List.of(parameters)))
                .build());
        ServerEnvelope response = harness.read();
        assertEquals(ServerEnvelope.PayloadCase.UPDATE_COMPLETED, response.getPayloadCase());
        assertEquals(affectedRows, response.getUpdateCompleted().getAffectedRows());
        assertTrue(response.getSerializedSize() <= 1024);
    }

    private static void assertCommunityValidationFailure(Harness harness, String code)
            throws Exception {
        ServerEnvelope response = harness.read();
        assertEquals(ServerEnvelope.PayloadCase.ERROR, response.getPayloadCase());
        assertEquals(code, response.getError().getCode());
        assertEquals(
                SessionState.SESSION_STATE_AUTO_COMMIT,
                response.getError().getSessionState());
    }

    private static List<ServerEnvelope> byRequest(List<ServerEnvelope> responses, String requestId) {
        return responses.stream()
                .filter(response -> response.getMeta().getRequestId().equals(requestId))
                .toList();
    }

    private static ClientEnvelope hello() {
        return ClientEnvelope.newBuilder()
                .setMeta(meta("hello"))
                .setHello(ClientHello.newBuilder()
                        .setRuntimeName("chat2db-rust")
                        .setRuntimeVersion("test")
                        .addSupportedVersions(ProtocolVersion.newBuilder().setMajor(1).setMinor(0))
                        .addAllRequiredCapabilities(List.of(
                                ProtocolLoop.PING_CAPABILITY,
                                ProtocolLoop.SHUTDOWN_CAPABILITY))
                        .setMaxReceiveFrameBytes(1024))
                .build();
    }

    private static boolean isCommunityCompatibilityConfigured() {
        String classpath = System.getenv(CommunityPluginRegistry.CLASSPATH_ENV);
        return classpath != null && !classpath.isBlank();
    }

    private static RequestMeta meta(String requestId) {
        return RequestMeta.newBuilder()
                .setRequestId(requestId)
                .setTraceId("trace-" + requestId)
                .build();
    }

    private static RequestMeta meta(String requestId, String sessionId) {
        return meta(requestId).toBuilder().setSessionId(sessionId).build();
    }

    private static Path h2DriverJar() throws IOException {
        Path directory = Path.of(System.getProperty("basedir"), "target", "test-drivers");
        try (var paths = Files.list(directory)) {
            Path jar = paths.filter(path -> path.getFileName().toString().startsWith("h2-"))
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .findFirst()
                    .orElseThrow();
            return jar.toRealPath();
        }
    }

    private static byte[] sha256(Path path) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (var input = Files.newInputStream(path)) {
            byte[] buffer = new byte[64 * 1024];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
            }
        }
        return digest.digest();
    }

    private static String expectedDriverId(String driverClass, byte[] artifactDigest)
            throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        digest.update("chat2db-jdbc-driver-v1\0".getBytes(StandardCharsets.UTF_8));
        digest.update(driverClass.getBytes(StandardCharsets.UTF_8));
        digest.update((byte) 0);
        digest.update(artifactDigest);
        return "sha256:" + java.util.HexFormat.of().formatHex(digest.digest());
    }

    private static final class Harness implements AutoCloseable {
        private final PipedInputStream runtimeInput = new PipedInputStream(64 * 1024);
        private final PipedOutputStream clientOutput = new PipedOutputStream(runtimeInput);
        private final PipedInputStream clientInput = new PipedInputStream(64 * 1024);
        private final PipedOutputStream runtimeOutput = new PipedOutputStream(clientInput);
        private final ByteArrayOutputStream diagnosticBytes = new ByteArrayOutputStream();
        private final ExecutorService executor = Executors.newFixedThreadPool(2);
        private final Future<Integer> loop;

        private Harness() throws IOException {
            ProtocolLoop protocolLoop = new ProtocolLoop(
                    new PrintStream(diagnosticBytes, true, StandardCharsets.UTF_8));
            loop = executor.submit(() -> protocolLoop.serve(runtimeInput, runtimeOutput));
        }

        private void send(ClientEnvelope request) throws IOException {
            FrameCodec.writeFrame(clientOutput, request);
        }

        private ServerEnvelope read()
                throws InterruptedException, ExecutionException, TimeoutException {
            Future<ServerEnvelope> response = executor.submit(() ->
                    FrameCodec.readFrame(clientInput, ServerEnvelope.parser()).orElseThrow());
            return response.get(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
        }

        private List<ServerEnvelope> read(int count) throws Exception {
            List<ServerEnvelope> responses = new ArrayList<>(count);
            for (int index = 0; index < count; index++) {
                responses.add(read());
            }
            return responses;
        }

        private int available() throws IOException {
            return clientInput.available();
        }

        private int awaitExit()
                throws InterruptedException, ExecutionException, TimeoutException {
            return loop.get(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
        }

        private String diagnostics() {
            return diagnosticBytes.toString(StandardCharsets.UTF_8);
        }

        @Override
        public void close() throws Exception {
            clientOutput.close();
            if (!loop.isDone()) {
                try {
                    loop.get(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
                } catch (ExecutionException ignored) {
                    // The test assertion remains authoritative.
                }
            }
            runtimeInput.close();
            runtimeOutput.close();
            clientInput.close();
            executor.shutdownNow();
            executor.awaitTermination(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
        }
    }
}

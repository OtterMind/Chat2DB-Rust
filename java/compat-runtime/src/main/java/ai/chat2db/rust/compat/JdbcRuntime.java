package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.BeginTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.CancelOperationRequest;
import ai.chat2db.rust.compat.protocol.v1.BuildCommunityCreateSchemaRequest;
import ai.chat2db.rust.compat.protocol.v1.CloseSessionRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabaseList;
import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKeyList;
import ai.chat2db.rust.compat.protocol.v1.CommunityFormattedSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunction;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionList;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionParameterList;
import ai.chat2db.rust.compat.protocol.v1.CommunityPluginCatalog;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKeyList;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedure;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureList;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureParameterList;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaList;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlAnalysis;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletion;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlValidation;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableColumnList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableIndexList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTrigger;
import ai.chat2db.rust.compat.protocol.v1.CommunityTriggerList;
import ai.chat2db.rust.compat.protocol.v1.CommunityViewList;
import ai.chat2db.rust.compat.protocol.v1.CommitTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.CreditsGranted;
import ai.chat2db.rust.compat.protocol.v1.DriverLoaded;
import ai.chat2db.rust.compat.protocol.v1.DriverUnloaded;
import ai.chat2db.rust.compat.protocol.v1.ExecuteQueryRequest;
import ai.chat2db.rust.compat.protocol.v1.ExecuteUpdateRequest;
import ai.chat2db.rust.compat.protocol.v1.FormatCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CompleteCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.GrantCreditsRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityFunctionRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityProcedureRequest;
import ai.chat2db.rust.compat.protocol.v1.GetCommunityTriggerRequest;
import ai.chat2db.rust.compat.protocol.v1.JdbcRow;
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
import ai.chat2db.rust.compat.protocol.v1.OperationCancelled;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.ParseCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.QueryCompleted;
import ai.chat2db.rust.compat.protocol.v1.QueryOptions;
import ai.chat2db.rust.compat.protocol.v1.QueryStarted;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.RollbackTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.RowBatch;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.SessionClosed;
import ai.chat2db.rust.compat.protocol.v1.SessionOpened;
import ai.chat2db.rust.compat.protocol.v1.TransactionCommitted;
import ai.chat2db.rust.compat.protocol.v1.TransactionRolledBack;
import ai.chat2db.rust.compat.protocol.v1.TransactionStarted;
import ai.chat2db.rust.compat.protocol.v1.UnloadDriverRequest;
import ai.chat2db.rust.compat.protocol.v1.UpdateCompleted;
import ai.chat2db.rust.compat.protocol.v1.ValidateCommunitySqlRequest;
import java.io.IOException;
import java.io.PrintStream;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicInteger;

/** JDBC protocol handlers and asynchronous typed query streaming. */
final class JdbcRuntime implements AutoCloseable {

    private static final int DEFAULT_BATCH_ROWS = 256;
    private static final int DEFAULT_BATCH_BYTES = 256 * 1024;
    private static final int QUERY_METADATA_ENVELOPE_OVERHEAD = 16;
    private static final Duration DEFAULT_SHUTDOWN_TIMEOUT = Duration.ofSeconds(5);
    private static final Duration DEFAULT_CANCELLATION_SETTLE_TIMEOUT = Duration.ofSeconds(5);

    private final DriverRegistry drivers = new DriverRegistry();
    private final SessionRegistry sessions = new SessionRegistry(drivers);
    private final OperationRegistry operations = new OperationRegistry();
    private final CommunityPluginRegistry community = CommunityPluginRegistry.openConfigured();
    private final CommunitySqlFormatter communitySqlFormatter = new CommunitySqlFormatter();
    private final ThreadPoolExecutor controlWorkers;
    private final ProtocolWriter writer;
    private final PrintStream diagnostics;
    private final Duration shutdownTimeout;
    private final Duration cancellationSettleTimeout;
    private volatile boolean closed;

    JdbcRuntime(ProtocolWriter writer, PrintStream diagnostics) {
        this(
                writer,
                diagnostics,
                DEFAULT_SHUTDOWN_TIMEOUT,
                DEFAULT_CANCELLATION_SETTLE_TIMEOUT);
    }

    JdbcRuntime(ProtocolWriter writer, PrintStream diagnostics, Duration shutdownTimeout) {
        this(writer, diagnostics, shutdownTimeout, DEFAULT_CANCELLATION_SETTLE_TIMEOUT);
    }

    JdbcRuntime(
            ProtocolWriter writer,
            PrintStream diagnostics,
            Duration shutdownTimeout,
            Duration cancellationSettleTimeout) {
        this.writer = writer;
        this.diagnostics = diagnostics;
        this.shutdownTimeout = shutdownTimeout;
        this.cancellationSettleTimeout = cancellationSettleTimeout;
        int workerCount = Math.max(2, Math.min(16, Runtime.getRuntime().availableProcessors()));
        AtomicInteger threadNumber = new AtomicInteger();
        ThreadFactory factory = task -> {
            Thread thread = new Thread(
                    task, "chat2db-jdbc-control-" + threadNumber.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        };
        controlWorkers = new ThreadPoolExecutor(
                workerCount,
                workerCount,
                0,
                TimeUnit.MILLISECONDS,
                new ArrayBlockingQueue<>(128),
                factory,
                new ThreadPoolExecutor.AbortPolicy());
    }

    void schedule(RequestMeta meta, JdbcCall call) throws RuntimeFailure {
        validateDeadline(meta);
        if (closed) {
            throw RuntimeFailure.conflict("database.runtime_closed", "the JDBC runtime is closing");
        }
        try {
            controlWorkers.execute(() -> {
                ServerEnvelope response;
                try {
                    validateDeadline(meta);
                    response = call.execute();
                } catch (RuntimeFailure failure) {
                    failure = attachSessionState(meta, failure);
                    diagnostics.printf(
                            "[compat-runtime] JDBC request failed code=%s request_id=%s%n",
                            failure.code(),
                            meta.getRequestId());
                    response = ProtocolResponses.failure(
                            meta, 0, failure, writer.peerMaximumFrameBytes());
                } catch (RuntimeException | LinkageError failure) {
                    RuntimeFailure translated = attachSessionState(
                            meta,
                            RuntimeFailure.internal(
                                    "database.control_internal_failure",
                                    "the JDBC control operation failed internally",
                                    failure));
                    response = ProtocolResponses.failure(
                            meta, 0, translated, writer.peerMaximumFrameBytes());
                }
                if (response != null) {
                    try {
                        writer.write(fitControlResponse(meta, response));
                    } catch (IOException failure) {
                        diagnostics.printf(
                                "[compat-runtime] asynchronous response failed code=protocol.write_failed request_id=%s%n",
                                meta.getRequestId());
                    }
                }
            });
        } catch (RuntimeException rejected) {
            throw RuntimeFailure.conflict(
                    "database.control_worker_unavailable",
                    "no JDBC control worker is currently available");
        }
    }

    ServerEnvelope loadDriver(RequestMeta meta, LoadDriverRequest request) throws RuntimeFailure {
        DriverRegistry.DriverDescriptor loaded = drivers.load(request);
        return terminal(meta)
                .setDriverLoaded(DriverLoaded.newBuilder()
                        .setDriverId(loaded.driverId())
                        .setDriverClass(loaded.driverClass())
                        .setArtifactCount(loaded.artifactCount()))
                .build();
    }

    ServerEnvelope unloadDriver(RequestMeta meta, UnloadDriverRequest request)
            throws RuntimeFailure {
        DriverRegistry.DriverDescriptor unloaded = drivers.unload(request.getDriverId());
        return terminal(meta)
                .setDriverUnloaded(
                        DriverUnloaded.newBuilder().setDriverId(unloaded.driverId()))
                .build();
    }

    ServerEnvelope openSession(RequestMeta meta, OpenSessionRequest request) throws RuntimeFailure {
        JdbcSession session = sessions.open(request);
        return terminal(meta)
                .setSessionOpened(SessionOpened.newBuilder()
                        .setSessionId(session.id())
                        .setDatabase(session.database())
                        .setReadOnly(session.defaultReadOnly())
                        .setSessionState(session.protocolState()))
                .build();
    }

    ServerEnvelope closeSession(
            RequestMeta meta, CloseSessionRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        sessions.close(sessionId);
        return terminal(meta)
                .setSessionClosed(SessionClosed.newBuilder()
                        .setSessionState(ai.chat2db.rust.compat.protocol.v1.SessionState.SESSION_STATE_CLOSED))
                .build();
    }

    ServerEnvelope beginTransaction(RequestMeta meta, BeginTransactionRequest request)
            throws RuntimeFailure {
        JdbcSession session = sessions.require(requireSessionId(meta));
        JdbcSession.TransactionDescriptor transaction =
                session.begin(request.getIsolation(), request.getReadOnly());
        return terminal(meta)
                .setTransactionStarted(TransactionStarted.newBuilder()
                        .setTransactionId(transaction.transactionId())
                        .setIsolation(transaction.isolation())
                        .setReadOnly(transaction.readOnly())
                        .setSessionState(session.protocolState()))
                .build();
    }

    ServerEnvelope commitTransaction(RequestMeta meta, CommitTransactionRequest request)
            throws RuntimeFailure {
        JdbcSession session = sessions.require(requireSessionId(meta));
        String transactionId = session.commit(request.getTransactionId());
        return terminal(meta)
                .setTransactionCommitted(TransactionCommitted.newBuilder()
                        .setTransactionId(transactionId)
                        .setSessionState(session.protocolState()))
                .build();
    }

    ServerEnvelope rollbackTransaction(RequestMeta meta, RollbackTransactionRequest request)
            throws RuntimeFailure {
        JdbcSession session = sessions.require(requireSessionId(meta));
        String transactionId = session.rollback(request.getTransactionId());
        return terminal(meta)
                .setTransactionRolledBack(TransactionRolledBack.newBuilder()
                        .setTransactionId(transactionId)
                        .setSessionState(session.protocolState()))
                .build();
    }

    void executeQuery(RequestMeta meta, ExecuteQueryRequest request) throws RuntimeFailure {
        validateDeadline(meta);
        validateSql(request.getSql());
        QueryLimits limits = QueryLimits.from(request.getOptions(), writer.peerMaximumFrameBytes());
        JdbcSession session = sessions.require(requireSessionId(meta));
        Optional<String> transactionId =
                request.hasTransactionId() ? Optional.of(request.getTransactionId()) : Optional.empty();
        OperationRegistry.QueryOperation operation =
                operations.register(session, meta, limits.initialCredits(), transactionId);
        operations.submit(operation, () -> runQuery(operation, request, limits));
    }

    void executeUpdate(RequestMeta meta, ExecuteUpdateRequest request)
            throws RuntimeFailure {
        validateDeadline(meta);
        validateSql(request.getSql());
        JdbcSession session = sessions.require(requireSessionId(meta));
        Optional<String> transactionId =
                request.hasTransactionId() ? Optional.of(request.getTransactionId()) : Optional.empty();
        OperationRegistry.QueryOperation operation =
                operations.register(session, meta, 0, transactionId);
        operations.submit(operation, () -> runUpdate(operation, request));
    }

    ServerEnvelope grantCredits(RequestMeta meta, GrantCreditsRequest request)
            throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                request.getTargetRequestId(),
                ProtocolLimits.MAX_DRIVER_ID_BYTES,
                "target_request_id");
        int accepted = operations.grantCredits(
                request.getTargetRequestId(), request.getBatchCredits());
        return terminal(meta)
                .setCreditsGranted(CreditsGranted.newBuilder()
                        .setAcceptedBatchCredits(accepted))
                .build();
    }

    ServerEnvelope cancelOperation(RequestMeta meta, CancelOperationRequest request)
            throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                request.getTargetRequestId(),
                ProtocolLimits.MAX_DRIVER_ID_BYTES,
                "target_request_id");
        return terminal(meta)
                .setOperationCancelled(OperationCancelled.newBuilder()
                        .setDisposition(operations.cancel(request.getTargetRequestId())))
                .build();
    }

    ServerEnvelope listCommunityPlugins(RequestMeta meta) throws RuntimeFailure {
        CommunityPluginCatalog catalog = community.catalog();
        return terminal(meta).setCommunityPluginCatalog(catalog).build();
    }

    ServerEnvelope listCommunitySchemas(
            RequestMeta meta, ListCommunitySchemasRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateSchemasRequest(
                request.getDatabaseType(), request.getDatabaseName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunitySchemaList schemas = invokeCommunitySchemas(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.schemas(
                        request.getDatabaseType(), connection, request.getDatabaseName()));
        return terminal(meta).setCommunitySchemaList(schemas).build();
    }

    static CommunitySchemaList invokeCommunitySchemas(
            JdbcSession session,
            String requestId,
            Optional<String> transactionId,
            CommunitySchemaInvocation invocation)
            throws RuntimeFailure {
        return invokeCommunityMetadata(session, requestId, transactionId, invocation::invoke);
    }

    private static <T> T invokeCommunityMetadata(
            JdbcSession session,
            String requestId,
            Optional<String> transactionId,
            CommunityMetadataInvocation<T> invocation)
            throws RuntimeFailure {
        Connection connection = session.claimOperation(requestId, transactionId);
        try {
            return invocation.invoke(connection);
        } catch (RuntimeFailure failure) {
            markCommunityFailure(session, failure);
            throw session.decorate(afterOperationClaim(failure));
        } catch (RuntimeException | LinkageError failure) {
            session.markQueryFailure();
            throw session.decorate(afterOperationClaim(RuntimeFailure.internal(
                    "community.metadata_failed",
                    "the Community metadata request failed internally",
                    failure)));
        } finally {
            session.finishOperation(requestId);
        }
    }

    private static void markCommunityFailure(JdbcSession session, RuntimeFailure failure) {
        if (failure.code().equals("community.sql_completion_connection_closed")
                || failure.code().equals("community.sql_completion_connection_state_failed")) {
            session.markBroken();
        } else {
            session.markQueryFailure();
        }
    }

    private static RuntimeFailure afterOperationClaim(RuntimeFailure failure) {
        if (failure.outcome() == OperationOutcome.OPERATION_OUTCOME_NOT_STARTED) {
            return failure.withOutcome(OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED);
        }
        return failure;
    }

    @FunctionalInterface
    interface CommunitySchemaInvocation {
        CommunitySchemaList invoke(Connection connection) throws RuntimeFailure;
    }

    @FunctionalInterface
    private interface CommunityMetadataInvocation<T> {
        T invoke(Connection connection) throws RuntimeFailure;
    }

    ServerEnvelope listCommunityDatabases(
            RequestMeta meta, ListCommunityDatabasesRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateDatabasesRequest(request.getDatabaseType());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityDatabaseList databases = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.databases(request.getDatabaseType(), connection));
        return terminal(meta).setCommunityDatabaseList(databases).build();
    }

    ServerEnvelope listCommunityTables(
            RequestMeta meta, ListCommunityTablesRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTablesRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTableNamePattern());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityTableList tables = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.tables(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTableNamePattern()));
        return terminal(meta).setCommunityTableList(tables).build();
    }

    ServerEnvelope listCommunityColumns(
            RequestMeta meta, ListCommunityColumnsRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTableObjectRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTableName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityTableColumnList columns = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.columns(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTableName()));
        return terminal(meta).setCommunityTableColumnList(columns).build();
    }

    ServerEnvelope listCommunityIndexes(
            RequestMeta meta, ListCommunityIndexesRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTableObjectRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTableName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityTableIndexList indexes = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.indexes(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTableName()));
        return terminal(meta).setCommunityTableIndexList(indexes).build();
    }

    ServerEnvelope listCommunityViews(
            RequestMeta meta, ListCommunityViewsRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateViewsRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getViewNamePattern());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityViewList views = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.views(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getViewNamePattern()));
        return terminal(meta).setCommunityViewList(views).build();
    }

    ServerEnvelope listCommunityImportedKeys(
            RequestMeta meta, ListCommunityTableKeysRequest request) throws RuntimeFailure {
        CommunityForeignKeyList keys = invokeCommunityTableKeys(
                meta,
                request,
                (connection, databaseType, databaseName, schemaName, tableName) ->
                        community.importedKeys(
                                databaseType,
                                connection,
                                databaseName,
                                schemaName,
                                tableName));
        return terminal(meta).setCommunityImportedKeyList(keys).build();
    }

    ServerEnvelope listCommunityExportedKeys(
            RequestMeta meta, ListCommunityTableKeysRequest request) throws RuntimeFailure {
        CommunityForeignKeyList keys = invokeCommunityTableKeys(
                meta,
                request,
                (connection, databaseType, databaseName, schemaName, tableName) ->
                        community.exportedKeys(
                                databaseType,
                                connection,
                                databaseName,
                                schemaName,
                                tableName));
        return terminal(meta).setCommunityExportedKeyList(keys).build();
    }

    ServerEnvelope listCommunityPrimaryKeys(
            RequestMeta meta, ListCommunityTableKeysRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTableObjectRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTableName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityPrimaryKeyList keys = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.primaryKeys(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTableName()));
        return terminal(meta).setCommunityPrimaryKeyList(keys).build();
    }

    private CommunityForeignKeyList invokeCommunityTableKeys(
            RequestMeta meta,
            ListCommunityTableKeysRequest request,
            CommunityForeignKeyInvocation invocation)
            throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTableObjectRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTableName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        return invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> invocation.invoke(
                        connection,
                        request.getDatabaseType(),
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTableName()));
    }

    @FunctionalInterface
    private interface CommunityForeignKeyInvocation {
        CommunityForeignKeyList invoke(
                Connection connection,
                String databaseType,
                String databaseName,
                String schemaName,
                String tableName)
                throws RuntimeFailure;
    }

    ServerEnvelope listCommunityFunctions(
            RequestMeta meta, ListCommunityFunctionsRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateProgrammabilityListRequest(
                request.getDatabaseType(), request.getDatabaseName(), request.getSchemaName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityFunctionList functions = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.functions(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName()));
        return terminal(meta).setCommunityFunctionList(functions).build();
    }

    ServerEnvelope getCommunityFunction(
            RequestMeta meta, GetCommunityFunctionRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateFunctionRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getFunctionName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityFunction function = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.function(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getFunctionName()));
        return terminal(meta).setCommunityFunction(function).build();
    }

    ServerEnvelope listCommunityFunctionParameters(
            RequestMeta meta, GetCommunityFunctionRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateFunctionRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getFunctionName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityFunctionParameterList parameters = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.functionParameters(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getFunctionName()));
        return terminal(meta).setCommunityFunctionParameterList(parameters).build();
    }

    ServerEnvelope listCommunityProcedures(
            RequestMeta meta, ListCommunityProceduresRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateProgrammabilityListRequest(
                request.getDatabaseType(), request.getDatabaseName(), request.getSchemaName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityProcedureList procedures = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.procedures(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName()));
        return terminal(meta).setCommunityProcedureList(procedures).build();
    }

    ServerEnvelope getCommunityProcedure(
            RequestMeta meta, GetCommunityProcedureRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateProcedureRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getProcedureName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityProcedure procedure = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.procedure(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getProcedureName()));
        return terminal(meta).setCommunityProcedure(procedure).build();
    }

    ServerEnvelope listCommunityProcedureParameters(
            RequestMeta meta, GetCommunityProcedureRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateProcedureRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getProcedureName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityProcedureParameterList parameters = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.procedureParameters(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getProcedureName()));
        return terminal(meta).setCommunityProcedureParameterList(parameters).build();
    }

    ServerEnvelope listCommunityTriggers(
            RequestMeta meta, ListCommunityTriggersRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateProgrammabilityListRequest(
                request.getDatabaseType(), request.getDatabaseName(), request.getSchemaName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityTriggerList triggers = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.triggers(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName()));
        return terminal(meta).setCommunityTriggerList(triggers).build();
    }

    ServerEnvelope getCommunityTrigger(
            RequestMeta meta, GetCommunityTriggerRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateTriggerRequest(
                request.getDatabaseType(),
                request.getDatabaseName(),
                request.getSchemaName(),
                request.getTriggerName());
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunityTrigger trigger = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.trigger(
                        request.getDatabaseType(),
                        connection,
                        request.getDatabaseName(),
                        request.getSchemaName(),
                        request.getTriggerName()));
        return terminal(meta).setCommunityTrigger(trigger).build();
    }

    ServerEnvelope buildCommunityCreateSchema(
            RequestMeta meta, BuildCommunityCreateSchemaRequest request)
            throws RuntimeFailure {
        return terminal(meta)
                .setCommunityBuiltSql(community.buildCreateSchema(
                        request.getDatabaseType(),
                        request.hasSchema() ? request.getSchema() : null))
                .build();
    }

    ServerEnvelope parseCommunitySql(RequestMeta meta, ParseCommunitySqlRequest request)
            throws RuntimeFailure {
        CommunitySqlAnalysis analysis =
                community.parse(request.getDatabaseType(), request.getSql());
        return terminal(meta).setCommunitySqlAnalysis(analysis).build();
    }

    ServerEnvelope validateCommunitySql(
            RequestMeta meta, ValidateCommunitySqlRequest request) throws RuntimeFailure {
        CommunitySqlValidation validation =
                community.validate(request.getDatabaseType(), request.getSql());
        return terminal(meta).setCommunitySqlValidation(validation).build();
    }

    ServerEnvelope formatCommunitySql(
            RequestMeta meta, FormatCommunitySqlRequest request) throws RuntimeFailure {
        CommunityFormattedSql formatted =
                communitySqlFormatter.format(request.getDatabaseType(), request.getSql());
        return terminal(meta).setCommunityFormattedSql(formatted).build();
    }

    ServerEnvelope completeCommunitySql(
            RequestMeta meta, CompleteCommunitySqlRequest request) throws RuntimeFailure {
        String sessionId = requireSessionId(meta);
        JdbcSession session = sessions.require(sessionId);
        community.validateSqlCompletionRequest(request);
        Optional<String> transactionId = request.hasTransactionId()
                ? Optional.of(request.getTransactionId())
                : Optional.empty();
        CommunitySqlCompletion completion = invokeCommunityMetadata(
                session,
                meta.getRequestId(),
                transactionId,
                connection -> community.completeSql(connection, request));
        return terminal(meta).setCommunitySqlCompletion(completion).build();
    }

    boolean communityCompatibilityConfigured() {
        return community.configured();
    }

    RuntimeFailure attachSessionState(RequestMeta meta, RuntimeFailure failure) {
        if (!meta.hasSessionId() || meta.getSessionId().isBlank()) {
            return failure;
        }
        try {
            return sessions.require(meta.getSessionId()).decorate(failure);
        } catch (RuntimeFailure sessionUnavailable) {
            return failure;
        }
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        long deadlineNanos = System.nanoTime() + shutdownTimeout.toNanos();
        controlWorkers.shutdownNow();
        boolean operationWorkersDrained = operations.close(remaining(deadlineNanos));
        boolean controlWorkersDrained = awaitControlWorkers(deadlineNanos);
        if (!operationWorkersDrained || !controlWorkersDrained) {
            diagnostics.println(
                    "[compat-runtime] JDBC shutdown incomplete code=database.workers_not_quiesced");
            return;
        }

        ExecutorService cleanup = Executors.newSingleThreadExecutor(task -> {
            Thread thread = new Thread(task, "chat2db-jdbc-shutdown-cleanup");
            thread.setDaemon(true);
            return thread;
        });
        Future<Boolean> cleaned = cleanup.submit(() -> {
            if (!sessions.closeAll()) {
                return false;
            }
            if (Thread.currentThread().isInterrupted() || System.nanoTime() >= deadlineNanos) {
                return false;
            }
            community.close();
            drivers.close();
            return true;
        });
        boolean resourcesReleased = false;
        try {
            long remainingNanos = deadlineNanos - System.nanoTime();
            if (remainingNanos > 0) {
                resourcesReleased = cleaned.get(remainingNanos, TimeUnit.NANOSECONDS);
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        } catch (ExecutionException | TimeoutException cleanupFailure) {
            // Resources stay owned until process exit.
        } finally {
            if (!resourcesReleased) {
                cleaned.cancel(true);
            }
            cleanup.shutdownNow();
        }
        if (!resourcesReleased) {
            diagnostics.println(
                    "[compat-runtime] JDBC shutdown incomplete code=database.resources_retained");
        }
    }

    void runQuery(
            OperationRegistry.QueryOperation operation,
            ExecuteQueryRequest request,
            QueryLimits limits) {
        QueryProgress progress = new QueryProgress();
        PreparedStatement statement = null;
        ResultSet resultSet = null;
        QueryCompletion completion = null;
        RuntimeFailure terminalFailure = null;
        boolean writerFailed = false;
        try {
            operation.checkDeadlineAndCancellation();
            statement = operation.connection().prepareStatement(request.getSql());
            operation.installStatement(statement);
            applyDeadline(statement, operation.meta());
            ValueCodec.bind(statement, request.getParametersList());
            operation.checkDeadlineAndCancellation();
            resultSet = statement.executeQuery();
            List<ai.chat2db.rust.compat.protocol.v1.JdbcColumn> columns =
                    ValueCodec.columns(
                            resultSet.getMetaData(),
                            queryMetadataBudget(operation.meta(), progress.current()));
            ServerEnvelope started = ProtocolResponses.response(
                            operation.meta(), progress.current(), false)
                    .setQueryStarted(QueryStarted.newBuilder().addAllColumns(columns))
                    .build();
            ensureFrameFits(started, "query_metadata");
            writer.write(started);
            progress.advance();
            completion = streamRows(operation, resultSet, columns, limits, progress);
        } catch (RuntimeFailure failure) {
            operation.session().markQueryFailure();
            terminalFailure = operation.session().decorate(failure);
        } catch (SQLException failure) {
            operation.session().markQueryFailure();
            try {
                operation.checkDeadlineAndCancellation();
                terminalFailure = operation.session().decorate(RuntimeFailure.database(
                        "database.query_failed",
                        "the query failed",
                        failure,
                        OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                        false));
            } catch (RuntimeFailure cancelled) {
                terminalFailure = operation.session().decorate(cancelled);
            }
        } catch (IOException failure) {
            operation.session().markBroken();
            writerFailed = true;
            diagnostics.printf(
                    "[compat-runtime] asynchronous response failed code=protocol.write_failed request_id=%s%n",
                    operation.requestId());
        } catch (RuntimeException failure) {
            operation.session().markBroken();
            terminalFailure = RuntimeFailure.internal(
                    "database.query_internal_failure",
                    "the query worker failed internally",
                    failure);
            terminalFailure = operation.session().decorate(terminalFailure);
        } finally {
            try {
                operation.sealAndAwaitCancellation(cancellationSettleTimeout);
            } catch (RuntimeFailure cancellation) {
                operation.session().markQueryFailure();
                RuntimeFailure decorated = operation.session().decorate(cancellation);
                if (terminalFailure == null
                        || decorated.code().equals("database.cancel_failed")
                        || decorated.code().equals("database.cancel_timeout")) {
                    terminalFailure = decorated;
                    completion = null;
                }
            }
            if (operation.hasPendingCancellation()) {
                ResultSet deferredResultSet = resultSet;
                PreparedStatement deferredStatement = statement;
                operations.deferFinish(operation, () -> {
                    RuntimeFailure cleanupFailure = closeQueryResources(
                            operation, deferredResultSet, deferredStatement);
                    if (cleanupFailure != null) {
                        diagnostics.printf(
                                "[compat-runtime] JDBC deferred cleanup failed code=%s request_id=%s%n",
                                cleanupFailure.code(),
                                operation.requestId());
                    }
                });
            } else {
                RuntimeFailure closeFailure =
                        closeQueryResources(operation, resultSet, statement);
                if (closeFailure != null) {
                    if (terminalFailure == null && !writerFailed) {
                        terminalFailure = closeFailure;
                        completion = null;
                    } else if (terminalFailure != null) {
                        terminalFailure = operation.session().decorate(terminalFailure);
                    }
                }
            }
        }

        try {
            if (writerFailed) {
                return;
            }
            if (terminalFailure != null) {
                writeQueryFailure(operation, progress.current(), terminalFailure);
                return;
            }
            QueryCompletion completedQuery = completion;
            if (completedQuery == null) {
                writeQueryFailure(
                        operation,
                        progress.current(),
                        operation.session().decorate(RuntimeFailure.internal(
                                "database.query_internal_failure",
                                "the query worker ended without a completion state",
                                null)));
                return;
            }
            ServerEnvelope completed = ProtocolResponses.response(
                            operation.meta(), progress.current(), true)
                    .setQueryCompleted(QueryCompleted.newBuilder()
                            .setRowCount(completedQuery.rowCount())
                            .setTruncatedByMaxRows(completedQuery.truncatedByMaxRows())
                            .setTruncatedByMaxResultBytes(
                                    completedQuery.truncatedByMaxResultBytes()))
                    .build();
            try {
                ensureFrameFits(completed, "query_completed");
                writer.write(completed, () -> operations.finish(operation));
                progress.advance();
            } catch (IOException | RuntimeFailure failure) {
                operation.session().markBroken();
                diagnostics.printf(
                        "[compat-runtime] asynchronous response failed code=protocol.write_failed request_id=%s%n",
                        operation.requestId());
            }
        } finally {
            operation.markTerminalResponseFinished();
            operations.finish(operation);
        }
    }

    void runUpdate(
            OperationRegistry.QueryOperation operation, ExecuteUpdateRequest request) {
        PreparedStatement statement = null;
        RuntimeFailure terminalFailure = null;
        Long affectedRows = null;
        boolean executionAttempted = false;
        try {
            operation.checkDeadlineAndCancellation();
            statement = operation.connection().prepareStatement(request.getSql());
            operation.installStatement(statement);
            applyDeadline(statement, operation.meta());
            ValueCodec.bind(statement, request.getParametersList());
            operation.checkDeadlineAndCancellation();
            executionAttempted = true;
            affectedRows = statement.executeLargeUpdate();
            operation.checkDeadlineAndCancellation();
            if (affectedRows < 0) {
                operation.session().markUpdateFailure();
                terminalFailure = operation.session().decorate(RuntimeFailure.internal(
                        "database.invalid_update_count",
                        "the driver returned a negative update count",
                        null).withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
                affectedRows = null;
            }
        } catch (RuntimeFailure failure) {
            if (executionAttempted) {
                operation.session().markUpdateFailure();
                failure = failure.withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN);
            }
            terminalFailure = operation.session().decorate(failure);
        } catch (SQLException failure) {
            RuntimeFailure cancellation = null;
            try {
                operation.checkDeadlineAndCancellation();
            } catch (RuntimeFailure cancelled) {
                cancellation = cancelled;
            }
            if (executionAttempted) {
                operation.session().markUpdateFailure();
            }
            terminalFailure = cancellation != null
                    ? (executionAttempted
                                    ? cancellation.withOutcome(
                                            OperationOutcome.OPERATION_OUTCOME_UNKNOWN)
                                    : cancellation)
                    : RuntimeFailure.database(
                            executionAttempted
                                    ? "database.update_outcome_unknown"
                                    : "database.update_failed",
                            executionAttempted
                                    ? "the update outcome is unknown"
                                    : "the update could not be started",
                            failure,
                            executionAttempted
                                    ? OperationOutcome.OPERATION_OUTCOME_UNKNOWN
                                    : OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                            false);
            terminalFailure = operation.session().decorate(terminalFailure);
        } catch (RuntimeException failure) {
            if (executionAttempted) {
                operation.session().markUpdateFailure();
            }
            RuntimeFailure translated = RuntimeFailure.internal(
                    "database.update_internal_failure",
                    "the update worker failed internally",
                    failure);
            if (executionAttempted) {
                translated = translated.withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN);
            }
            terminalFailure = operation.session().decorate(translated);
        } finally {
            try {
                operation.sealAndAwaitCancellation(cancellationSettleTimeout);
            } catch (RuntimeFailure cancellation) {
                RuntimeFailure translated = cancellation;
                if (executionAttempted) {
                    operation.session().markUpdateFailure();
                    translated = translated.withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN);
                }
                terminalFailure = operation.session().decorate(translated);
                affectedRows = null;
            }
            if (operation.hasPendingCancellation()) {
                PreparedStatement deferredStatement = statement;
                operations.deferFinish(operation, () -> {
                    RuntimeFailure cleanupFailure =
                            closeStatement(operation, deferredStatement, "update");
                    if (cleanupFailure != null) {
                        diagnostics.printf(
                                "[compat-runtime] JDBC deferred cleanup failed code=%s request_id=%s%n",
                                cleanupFailure.code(),
                                operation.requestId());
                    }
                });
            } else {
                RuntimeFailure closeFailure = closeStatement(operation, statement, "update");
                if (closeFailure != null) {
                    if (terminalFailure == null) {
                        terminalFailure = closeFailure;
                        affectedRows = null;
                    } else {
                        terminalFailure = operation.session().decorate(terminalFailure);
                    }
                }
            }
        }

        ServerEnvelope response = terminalFailure == null
                ? terminal(operation.meta())
                        .setUpdateCompleted(UpdateCompleted.newBuilder()
                                .setAffectedRows(affectedRows == null ? 0 : affectedRows))
                        .build()
                : ProtocolResponses.failure(
                        operation.meta(),
                        0,
                        terminalFailure,
                        writer.peerMaximumFrameBytes());
        try {
            ensureFrameFits(response, terminalFailure == null ? "update_completed" : "update_error");
            writer.write(response, () -> operations.finish(operation));
        } catch (IOException | RuntimeFailure failure) {
            operation.session().markBroken();
            diagnostics.printf(
                    "[compat-runtime] asynchronous response failed code=protocol.write_failed request_id=%s%n",
                    operation.requestId());
        } finally {
            operation.markTerminalResponseFinished();
            operations.finish(operation);
        }
    }

    private RuntimeFailure closeQueryResources(
            OperationRegistry.QueryOperation operation,
            ResultSet resultSet,
            PreparedStatement statement) {
        RuntimeFailure failure = closeResultSet(operation, resultSet);
        RuntimeFailure statementFailure = closeStatement(operation, statement, "query");
        return failure != null ? failure : statementFailure;
    }

    private RuntimeFailure closeResultSet(
            OperationRegistry.QueryOperation operation, ResultSet resultSet) {
        if (resultSet == null) {
            return null;
        }
        try {
            resultSet.close();
            return null;
        } catch (SQLException closeFailure) {
            operation.session().markBroken();
            return operation.session().decorate(RuntimeFailure.database(
                    "database.result_set_close_failed",
                    "the query result set could not be closed cleanly",
                    closeFailure,
                    OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                    false));
        } catch (RuntimeException closeFailure) {
            operation.session().markBroken();
            return operation.session().decorate(RuntimeFailure.internal(
                            "database.result_set_close_failed",
                            "the query result set could not be closed cleanly",
                            closeFailure)
                    .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
        }
    }

    private RuntimeFailure closeStatement(
            OperationRegistry.QueryOperation operation,
            PreparedStatement statement,
            String operationKind) {
        if (statement == null) {
            return null;
        }
        operation.clearStatement(statement);
        try {
            statement.close();
            return null;
        } catch (SQLException closeFailure) {
            operation.session().markBroken();
            return operation.session().decorate(RuntimeFailure.database(
                    "database.statement_close_failed",
                    "the " + operationKind + " statement could not be closed cleanly",
                    closeFailure,
                    OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                    false));
        } catch (RuntimeException closeFailure) {
            operation.session().markBroken();
            return operation.session().decorate(RuntimeFailure.internal(
                            "database.statement_close_failed",
                            "the " + operationKind + " statement could not be closed cleanly",
                            closeFailure)
                    .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
        }
    }

    QueryCompletion streamRows(
            OperationRegistry.QueryOperation operation,
            ResultSet resultSet,
            List<ai.chat2db.rust.compat.protocol.v1.JdbcColumn> columns,
            QueryLimits limits,
            QueryProgress progress)
            throws SQLException, RuntimeFailure, IOException {
        long rowCount = 0;
        long acceptedRowBytes = 0;
        boolean truncatedByRows = false;
        boolean truncatedByBytes = false;
        JdbcRow pendingRow = null;
        boolean exhausted = false;

        while (!exhausted) {
            operation.awaitCredit();
            boolean creditConsumed = false;
            List<JdbcRow> batch = new ArrayList<>(limits.batchRows());
            long batchOffset = rowCount;
            try {
                while (batch.size() < limits.batchRows()) {
                    operation.checkDeadlineAndCancellation();
                    if (limits.maxRows() != 0 && rowCount >= limits.maxRows()) {
                        boolean hasAdditional = pendingRow != null || resultSet.next();
                        truncatedByRows = hasAdditional;
                        exhausted = true;
                        break;
                    }
                    long remainingResultBytes = limits.maxResultBytes() - acceptedRowBytes;
                    if (remainingResultBytes == 0) {
                        boolean hasAdditional = pendingRow != null || resultSet.next();
                        truncatedByBytes = hasAdditional;
                        exhausted = true;
                        break;
                    }

                    JdbcRow row;
                    if (pendingRow != null) {
                        row = pendingRow;
                        pendingRow = null;
                    } else {
                        if (!resultSet.next()) {
                            exhausted = true;
                            break;
                        }
                        RowRead read = readRow(
                                resultSet,
                                columns,
                                limits.batchBytes(),
                                remainingResultBytes);
                        if (read.truncatedByResultBytes()) {
                            truncatedByBytes = true;
                            exhausted = true;
                            break;
                        }
                        row = read.row();
                    }

                    if (!batch.isEmpty()
                            && encodedBatchSize(batchOffset, batch, row) > limits.batchBytes()) {
                        pendingRow = row;
                        break;
                    }
                    batch.add(row);
                    rowCount++;
                    acceptedRowBytes += row.getSerializedSize();

                    if (limits.maxRows() != 0 && rowCount >= limits.maxRows()) {
                        truncatedByRows = resultSet.next();
                        exhausted = true;
                        break;
                    }
                    if (acceptedRowBytes == limits.maxResultBytes()) {
                        truncatedByBytes = resultSet.next();
                        exhausted = true;
                        break;
                    }
                    if (batch.size() >= limits.batchRows() && isLastRow(resultSet)) {
                        exhausted = true;
                    }
                }

                if (batch.isEmpty()) {
                    operation.returnCredit();
                    creditConsumed = true;
                    break;
                }
                writeBatch(operation, progress, batchOffset, batch);
                creditConsumed = true;
            } finally {
                if (!creditConsumed) {
                    operation.returnCredit();
                }
            }
        }
        return new QueryCompletion(
                rowCount, truncatedByRows, truncatedByBytes);
    }

    private void writeBatch(
            OperationRegistry.QueryOperation operation,
            QueryProgress progress,
            long startOffset,
            List<JdbcRow> rows)
            throws RuntimeFailure, IOException {
        RowBatch batch = RowBatch.newBuilder()
                .setStartRowOffset(startOffset)
                .addAllRows(rows)
                .build();
        ServerEnvelope envelope = ProtocolResponses.response(
                        operation.meta(), progress.current(), false)
                .setRowBatch(batch)
                .build();
        ensureFrameFits(envelope, "row_batch");
        operation.checkDeadlineAndCancellation();
        writer.write(envelope);
        operation.consumeReservedCredit();
        progress.advance();
    }

    private static RowRead readRow(
            ResultSet resultSet,
            List<ai.chat2db.rust.compat.protocol.v1.JdbcColumn> columns,
            int maximumBatchBytes,
            long remainingResultBytes)
            throws SQLException, RuntimeFailure {
        JdbcRow.Builder row = JdbcRow.newBuilder();
        for (ai.chat2db.rust.compat.protocol.v1.JdbcColumn column : columns) {
            int currentResultBytes = row.build().getSerializedSize();
            int currentBatchBytes = RowBatch.newBuilder().addRows(row.build()).build().getSerializedSize();
            long resultContentBudget = Math.max(0, remainingResultBytes - currentResultBytes);
            int batchContentBudget = Math.max(0, maximumBatchBytes - currentBatchBytes);
            int contentBudget = (int) Math.min(
                    ProtocolLimits.MAX_SCALAR_BYTES,
                    Math.min(resultContentBudget, batchContentBudget));
            try {
                row.addValues(ValueCodec.read(resultSet, column, contentBudget));
            } catch (ValueCodec.ValueLimitExceeded exceeded) {
                if (resultContentBudget <= batchContentBudget
                        && resultContentBudget <= ProtocolLimits.MAX_SCALAR_BYTES) {
                    return RowRead.resultLimit();
                }
                if (batchContentBudget <= ProtocolLimits.MAX_SCALAR_BYTES) {
                    throw RuntimeFailure.limit("single_cell_row_batch", maximumBatchBytes);
                }
                throw RuntimeFailure.limit(exceeded.field(), ProtocolLimits.MAX_SCALAR_BYTES);
            }
            JdbcRow partial = row.build();
            if (partial.getSerializedSize() > remainingResultBytes) {
                return RowRead.resultLimit();
            }
            ensureSingleRowFits(partial, maximumBatchBytes);
        }
        return RowRead.row(row.build());
    }

    private static boolean isLastRow(ResultSet resultSet) {
        try {
            return resultSet.isLast();
        } catch (SQLException | RuntimeException unsupported) {
            return false;
        }
    }

    private static void ensureSingleRowFits(JdbcRow row, int maximumBytes)
            throws RuntimeFailure {
        for (ai.chat2db.rust.compat.protocol.v1.JdbcValue value : row.getValuesList()) {
            if (value.getSerializedSize() > maximumBytes) {
                throw RuntimeFailure.limit("single_cell_row_batch", maximumBytes);
            }
        }
        RowBatch single = RowBatch.newBuilder().addRows(row).build();
        if (single.getSerializedSize() > maximumBytes) {
            throw RuntimeFailure.limit("single_row_batch", maximumBytes);
        }
    }

    private static int encodedBatchSize(long startOffset, List<JdbcRow> rows, JdbcRow additional) {
        return RowBatch.newBuilder()
                .setStartRowOffset(startOffset)
                .addAllRows(rows)
                .addRows(additional)
                .build()
                .getSerializedSize();
    }

    private void ensureFrameFits(ServerEnvelope envelope, String field) throws RuntimeFailure {
        int maximum = Math.min(FrameCodec.MAX_FRAME_BYTES, writer.peerMaximumFrameBytes());
        if (envelope.getSerializedSize() > maximum) {
            throw RuntimeFailure.limit(field, maximum);
        }
    }

    private int queryMetadataBudget(RequestMeta meta, long sequence) throws RuntimeFailure {
        int maximum = Math.min(FrameCodec.MAX_FRAME_BYTES, writer.peerMaximumFrameBytes());
        int fixedBytes = ProtocolResponses.response(meta, sequence, false)
                .build()
                .getSerializedSize();
        int budget = maximum - fixedBytes - QUERY_METADATA_ENVELOPE_OVERHEAD;
        if (budget <= 0) {
            throw RuntimeFailure.limit("query_metadata", maximum);
        }
        return budget;
    }

    private void writeQueryFailure(
            OperationRegistry.QueryOperation operation,
            long sequence,
            RuntimeFailure failure) {
        try {
            ServerEnvelope response = ProtocolResponses.failure(
                    operation.meta(), sequence, failure, writer.peerMaximumFrameBytes());
            ensureFrameFits(response, "query_error");
            writer.write(response, () -> operations.finish(operation));
        } catch (IOException | RuntimeFailure writeFailure) {
            operation.session().markBroken();
            diagnostics.printf(
                    "[compat-runtime] asynchronous response failed code=protocol.write_failed request_id=%s%n",
                    operation.requestId());
        }
    }

    private static void validateSql(String sql) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(sql, ProtocolLimits.MAX_SQL_BYTES, "sql");
    }

    private ServerEnvelope fitControlResponse(RequestMeta meta, ServerEnvelope response) {
        if (response.getSerializedSize() <= writer.peerMaximumFrameBytes()) {
            return response;
        }
        RuntimeFailure failure = attachSessionState(
                meta,
                RuntimeFailure.limit("control_response_frame", writer.peerMaximumFrameBytes()));
        return ProtocolResponses.failure(
                meta, 0, failure, writer.peerMaximumFrameBytes());
    }

    private boolean awaitControlWorkers(long deadlineNanos) {
        long remainingNanos = deadlineNanos - System.nanoTime();
        try {
            if (remainingNanos > 0) {
                controlWorkers.awaitTermination(remainingNanos, TimeUnit.NANOSECONDS);
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return false;
        }
        return controlWorkers.isTerminated();
    }

    private static Duration remaining(long deadlineNanos) {
        return Duration.ofNanos(Math.max(0, deadlineNanos - System.nanoTime()));
    }

    private static void applyDeadline(PreparedStatement statement, RequestMeta meta)
            throws SQLException, RuntimeFailure {
        if (!meta.hasDeadlineUnixMillis()) {
            return;
        }
        long remainingMillis = meta.getDeadlineUnixMillis() - System.currentTimeMillis();
        if (remainingMillis <= 0) {
            throw RuntimeFailure.deadline("the operation deadline elapsed before execution");
        }
        long seconds = Math.max(1, (remainingMillis + 999) / 1000);
        statement.setQueryTimeout((int) Math.min(Integer.MAX_VALUE, seconds));
    }

    private static void validateDeadline(RequestMeta meta) throws RuntimeFailure {
        if (meta.hasDeadlineUnixMillis()
                && meta.getDeadlineUnixMillis() <= System.currentTimeMillis()) {
            throw RuntimeFailure.deadline("the operation deadline elapsed before execution");
        }
    }

    private static String requireSessionId(RequestMeta meta) throws RuntimeFailure {
        if (!meta.hasSessionId()) {
            throw RuntimeFailure.validation(
                    "session.id_required", "RequestMeta.session_id is required for this operation");
        }
        ProtocolLimits.requireNonBlankUtf8(
                meta.getSessionId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "session_id");
        return meta.getSessionId();
    }

    private static ServerEnvelope.Builder terminal(RequestMeta meta) {
        return ProtocolResponses.response(meta, 0, true);
    }

    record QueryLimits(
            int batchRows,
            int batchBytes,
            int initialCredits,
            long maxRows,
            long maxResultBytes) {

        private static QueryLimits from(QueryOptions options, int peerMaximumFrameBytes)
                throws RuntimeFailure {
            int batchRows = options.getTargetBatchRows() == 0
                    ? DEFAULT_BATCH_ROWS
                    : options.getTargetBatchRows();
            if (batchRows <= 0 || batchRows > ProtocolLimits.MAX_BATCH_ROWS) {
                throw RuntimeFailure.validation(
                        "query.invalid_target_batch_rows",
                        "target_batch_rows must be between 1 and " + ProtocolLimits.MAX_BATCH_ROWS);
            }

            int requestedBytes = options.getTargetBatchBytes() == 0
                    ? DEFAULT_BATCH_BYTES
                    : options.getTargetBatchBytes();
            if (requestedBytes < 1024 || requestedBytes > ProtocolLimits.MAX_BATCH_BYTES) {
                throw RuntimeFailure.validation(
                        "query.invalid_target_batch_bytes",
                        "target_batch_bytes must be between 1024 and "
                                + ProtocolLimits.MAX_BATCH_BYTES);
            }
            int frameBudget = Math.max(1, peerMaximumFrameBytes - 512);
            int batchBytes = Math.min(requestedBytes, frameBudget);

            int initialCredits = options.getInitialBatchCredits();
            if (initialCredits < 0
                    || initialCredits > ProtocolLimits.MAX_CREDIT_GRANT) {
                throw RuntimeFailure.validation(
                        "query.invalid_initial_credits",
                        "initial_batch_credits exceeds the per-request credit grant limit");
            }
            long maxRows = options.getMaxRows();
            if (maxRows < 0) {
                maxRows = Long.MAX_VALUE;
            }
            long requestedResultBytes = options.getMaxResultBytes();
            if (Long.compareUnsigned(requestedResultBytes, ProtocolLimits.MAX_RESULT_BYTES) > 0) {
                throw RuntimeFailure.validation(
                        "query.invalid_max_result_bytes",
                        "max_result_bytes must not exceed " + ProtocolLimits.MAX_RESULT_BYTES);
            }
            long maxResultBytes = requestedResultBytes == 0
                    ? ProtocolLimits.DEFAULT_RESULT_BYTES
                    : requestedResultBytes;
            return new QueryLimits(
                    batchRows, batchBytes, initialCredits, maxRows, maxResultBytes);
        }
    }

    record QueryCompletion(
            long rowCount,
            boolean truncatedByMaxRows,
            boolean truncatedByMaxResultBytes) {
    }

    private record RowRead(JdbcRow row, boolean truncatedByResultBytes) {
        private static RowRead row(JdbcRow row) {
            return new RowRead(row, false);
        }

        private static RowRead resultLimit() {
            return new RowRead(null, true);
        }
    }

    static final class QueryProgress {
        private long nextSequence;

        long current() {
            return nextSequence;
        }

        void advance() {
            nextSequence++;
        }
    }

    @FunctionalInterface
    interface JdbcCall {
        ServerEnvelope execute() throws RuntimeFailure;
    }
}

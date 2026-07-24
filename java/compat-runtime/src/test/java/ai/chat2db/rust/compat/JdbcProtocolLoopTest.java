package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.BeginTransactionRequest;
import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.CancelOperationRequest;
import ai.chat2db.rust.compat.protocol.v1.ClientEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ClientHello;
import ai.chat2db.rust.compat.protocol.v1.CloseSessionRequest;
import ai.chat2db.rust.compat.protocol.v1.ConnectionProperty;
import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.ExecuteQueryRequest;
import ai.chat2db.rust.compat.protocol.v1.ExecuteUpdateRequest;
import ai.chat2db.rust.compat.protocol.v1.ErrorCategory;
import ai.chat2db.rust.compat.protocol.v1.GrantCreditsRequest;
import ai.chat2db.rust.compat.protocol.v1.JdbcParameter;
import ai.chat2db.rust.compat.protocol.v1.JdbcValue;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
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
    void loadsExternalH2AndStreamsTypedRowsWithTransactionsAndBackpressure()
            throws Exception {
        Path h2Jar = h2DriverJar();
        byte[] digest = sha256(h2Jar);

        try (Harness harness = new Harness()) {
            harness.send(hello());
            ServerEnvelope hello = harness.read();
            assertEquals(ServerEnvelope.PayloadCase.HELLO, hello.getPayloadCase());
            assertEquals(
                    List.of(
                            ProtocolLoop.PING_CAPABILITY,
                            ProtocolLoop.SHUTDOWN_CAPABILITY,
                            ProtocolLoop.EXTERNAL_DRIVER_CAPABILITY,
                            ProtocolLoop.JDBC_SESSION_CAPABILITY,
                            ProtocolLoop.TYPED_QUERY_CAPABILITY,
                            ProtocolLoop.CREDIT_FLOW_CAPABILITY,
                            ProtocolLoop.OPERATION_CANCEL_CAPABILITY,
                            ProtocolLoop.JDBC_UPDATE_CAPABILITY,
                            ProtocolLoop.LOCAL_TRANSACTION_CAPABILITY),
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
                    .setMeta(meta("begin", sessionId))
                    .setBeginTransaction(BeginTransactionRequest.newBuilder()
                            .setIsolation(TransactionIsolation.TRANSACTION_ISOLATION_READ_COMMITTED))
                    .build());
            ServerEnvelope transaction = harness.read();
            assertEquals(
                    SessionState.SESSION_STATE_TRANSACTION_ACTIVE,
                    transaction.getTransactionStarted().getSessionState());
            String transactionId = transaction.getTransactionStarted().getTransactionId();

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

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

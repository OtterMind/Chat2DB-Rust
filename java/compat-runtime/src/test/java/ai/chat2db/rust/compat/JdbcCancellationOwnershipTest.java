package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.ExecuteUpdateRequest;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import com.google.protobuf.ByteString;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.time.Duration;
import java.util.Optional;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class JdbcCancellationOwnershipTest {

    @Test
    void hungCancelReturnsTerminalBeforeReleasingStatementSessionOrDriver(
            @TempDir Path temporaryDirectory) throws Exception {
        CountDownLatch executionStarted = new CountDownLatch(1);
        CountDownLatch cancellationStarted = new CountDownLatch(1);
        CountDownLatch releaseCancellation = new CountDownLatch(1);
        AtomicInteger statementCloseCalls = new AtomicInteger();
        AtomicInteger connectionCloseCalls = new AtomicInteger();
        AtomicBoolean connectionClosed = new AtomicBoolean();
        PreparedStatement statement = (PreparedStatement) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {PreparedStatement.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "executeLargeUpdate" -> {
                        executionStarted.countDown();
                        if (!cancellationStarted.await(2, TimeUnit.SECONDS)) {
                            throw new IllegalStateException("cancellation did not start");
                        }
                        yield 1L;
                    }
                    case "cancel" -> {
                        cancellationStarted.countDown();
                        releaseCancellation.await();
                        yield null;
                    }
                    case "close" -> {
                        statementCloseCalls.incrementAndGet();
                        yield null;
                    }
                    default -> defaultValue(method.getReturnType());
                });
        Connection connection = (Connection) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "prepareStatement" -> statement;
                    case "getAutoCommit" -> true;
                    case "close" -> {
                        connectionCloseCalls.incrementAndGet();
                        connectionClosed.set(true);
                        yield null;
                    }
                    case "isClosed" -> connectionClosed.get();
                    default -> defaultValue(method.getReturnType());
                });

        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        DriverRegistry driverRegistry = new DriverRegistry(snapshotRoot);
        DriverRegistry.DriverDescriptor driver = loadH2(driverRegistry);
        JdbcSession session = new JdbcSession(
                "cancel-session",
                connection,
                driverRegistry.acquire(driver.driverId()),
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        RequestMeta meta = RequestMeta.newBuilder()
                .setRequestId("hung-cancel")
                .setTraceId("trace-hung-cancel")
                .build();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        ProtocolWriter writer = new ProtocolWriter(output);
        JdbcRuntime runtime = new JdbcRuntime(
                writer, System.err, Duration.ofSeconds(2), Duration.ofMillis(100));
        ExecutorService executor = Executors.newSingleThreadExecutor();
        OperationRegistry operations = operations(runtime);
        OperationRegistry.QueryOperation operation =
                operations.register(session, meta, 0, Optional.empty());
        Future<?> update = executor.submit(() -> runtime.runUpdate(
                operation,
                ExecuteUpdateRequest.newBuilder()
                        .setSql("UPDATE test SET value = 1")
                        .build()));

        try {
            assertTrue(executionStarted.await(2, TimeUnit.SECONDS));
            assertEquals(
                    CancelDisposition.CANCEL_DISPOSITION_ACCEPTED,
                    operations.cancel(meta.getRequestId()));
            assertTrue(cancellationStarted.await(2, TimeUnit.SECONDS));

            update.get(2, TimeUnit.SECONDS);
            ServerEnvelope terminal = FrameCodec.readFrame(
                            new ByteArrayInputStream(output.toByteArray()), ServerEnvelope.parser())
                    .orElseThrow();
            assertEquals(ServerEnvelope.PayloadCase.ERROR, terminal.getPayloadCase());
            assertEquals("database.cancel_timeout", terminal.getError().getCode());
            assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, terminal.getError().getOutcome());
            assertEquals(SessionState.SESSION_STATE_BROKEN, terminal.getError().getSessionState());
            assertEquals(0, statementCloseCalls.get());
            RuntimeFailure terminalCredit = assertThrows(
                    RuntimeFailure.class,
                    () -> operations.grantCredits(meta.getRequestId(), 1));
            assertEquals("operation.not_active", terminalCredit.code());

            RuntimeFailure busy = assertThrows(RuntimeFailure.class, session::close);
            assertEquals("session.operation_in_progress", busy.code());
            RuntimeFailure inUse = assertThrows(
                    RuntimeFailure.class, () -> driverRegistry.unload(driver.driverId()));
            assertEquals("driver.in_use", inUse.code());

            releaseCancellation.countDown();
            long cleanupDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
            while ((statementCloseCalls.get() == 0 || session.activeOperationId().isPresent())
                    && System.nanoTime() < cleanupDeadline) {
                TimeUnit.MILLISECONDS.sleep(5);
            }
            assertEquals(1, statementCloseCalls.get());
            assertTrue(session.activeOperationId().isEmpty());

            session.close();
            assertEquals(1, connectionCloseCalls.get());
            driverRegistry.unload(driver.driverId());
            try (var snapshots = Files.list(snapshotRoot)) {
                assertEquals(0, snapshots.count());
            }
        } finally {
            releaseCancellation.countDown();
            executor.shutdownNow();
            executor.awaitTermination(5, TimeUnit.SECONDS);
            runtime.close();
            writer.close();
            driverRegistry.close();
        }
    }

    private static OperationRegistry operations(JdbcRuntime runtime) throws Exception {
        Field field = JdbcRuntime.class.getDeclaredField("operations");
        field.setAccessible(true);
        return (OperationRegistry) field.get(runtime);
    }

    private static DriverRegistry.DriverDescriptor loadH2(DriverRegistry registry)
            throws Exception {
        Path jar = h2DriverJar();
        return registry.load(LoadDriverRequest.newBuilder()
                .setDriverClass("org.h2.Driver")
                .addArtifacts(DriverArtifact.newBuilder()
                        .setPath(jar.toString())
                        .setSha256(ByteString.copyFrom(sha256(jar))))
                .build());
    }

    private static Path h2DriverJar() throws Exception {
        Path directory = Path.of(System.getProperty("basedir"), "target", "test-drivers");
        try (var paths = Files.list(directory)) {
            return paths.filter(path -> path.getFileName().toString().startsWith("h2-"))
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .findFirst()
                    .orElseThrow()
                    .toRealPath();
        }
    }

    private static byte[] sha256(Path path) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (var input = Files.newInputStream(path)) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
            }
        }
        return digest.digest();
    }

    private static Object defaultValue(Class<?> type) {
        if (!type.isPrimitive()) {
            return null;
        }
        if (type == boolean.class) {
            return false;
        }
        if (type == byte.class) {
            return (byte) 0;
        }
        if (type == short.class) {
            return (short) 0;
        }
        if (type == int.class) {
            return 0;
        }
        if (type == long.class) {
            return 0L;
        }
        if (type == float.class) {
            return 0F;
        }
        if (type == double.class) {
            return 0D;
        }
        if (type == char.class) {
            return '\0';
        }
        return null;
    }
}

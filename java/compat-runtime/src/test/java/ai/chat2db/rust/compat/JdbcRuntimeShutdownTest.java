package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.QuiescenceSnapshot;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.time.Duration;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

class JdbcRuntimeShutdownTest {

    @Test
    void quiescenceSnapshotReportsSessionsOperationsAndControlTasks() throws Exception {
        Connection connection = (Connection) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getAutoCommit" -> true;
                    default -> defaultValue(method.getReturnType());
                });
        JdbcSession session = new JdbcSession(
                "snapshot-session",
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        CountDownLatch controlStarted = new CountDownLatch(1);
        CountDownLatch releaseControl = new CountDownLatch(1);
        ProtocolWriter writer = new ProtocolWriter(new ByteArrayOutputStream());
        JdbcRuntime runtime = new JdbcRuntime(
                writer,
                new PrintStream(new ByteArrayOutputStream(), true, StandardCharsets.UTF_8));
        OperationRegistry operations = field(runtime, "operations", OperationRegistry.class);
        SessionRegistry sessions = field(runtime, "sessions", SessionRegistry.class);
        @SuppressWarnings("unchecked")
        Map<String, JdbcSession> ownedSessions = field(sessions, "sessions", Map.class);
        synchronized (sessions) {
            ownedSessions.put(session.id(), session);
        }
        OperationRegistry.QueryOperation operation = operations.register(
                session,
                RequestMeta.newBuilder()
                        .setRequestId("snapshot-operation")
                        .setTraceId("trace-snapshot-operation")
                        .build(),
                0,
                Optional.empty());

        try {
            runtime.schedule(
                    RequestMeta.newBuilder()
                            .setRequestId("snapshot-control")
                            .setTraceId("trace-snapshot-control")
                            .build(),
                    () -> {
                        controlStarted.countDown();
                        try {
                            releaseControl.await();
                        } catch (InterruptedException interrupted) {
                            Thread.currentThread().interrupt();
                        }
                        return null;
                    });
            assertTrue(controlStarted.await(2, TimeUnit.SECONDS));

            QuiescenceSnapshot snapshot = runtime.quiescenceSnapshot();
            assertEquals(1, snapshot.getActiveSessions());
            assertEquals(1, snapshot.getActiveOperations());
            assertEquals(1, snapshot.getActiveControlTasks());
            assertEquals(0, snapshot.getQueuedControlTasks());
            assertEquals(0, snapshot.getPendingOutboundFrames());
        } finally {
            releaseControl.countDown();
            operations.finish(operation);
            runtime.close();
            writer.close();
        }
    }

    @Test
    void nonQuiescedWorkerPreventsConcurrentConnectionCleanup() throws Exception {
        AtomicInteger connectionCloseCalls = new AtomicInteger();
        Connection connection = (Connection) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getAutoCommit" -> true;
                    case "close" -> {
                        connectionCloseCalls.incrementAndGet();
                        yield null;
                    }
                    default -> defaultValue(method.getReturnType());
                });
        JdbcSession session = new JdbcSession(
                "session",
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        AtomicBoolean release = new AtomicBoolean();
        CountDownLatch started = new CountDownLatch(1);
        ByteArrayOutputStream diagnostics = new ByteArrayOutputStream();
        ProtocolWriter writer = new ProtocolWriter(new ByteArrayOutputStream());
        JdbcRuntime runtime = new JdbcRuntime(
                writer,
                new PrintStream(diagnostics, true, StandardCharsets.UTF_8),
                Duration.ofMillis(100));
        OperationRegistry operations = field(runtime, "operations", OperationRegistry.class);
        SessionRegistry sessions = field(runtime, "sessions", SessionRegistry.class);
        @SuppressWarnings("unchecked")
        Map<String, JdbcSession> ownedSessions =
                field(sessions, "sessions", Map.class);
        ownedSessions.put(session.id(), session);

        try {
            OperationRegistry.QueryOperation operation = operations.register(
                    session,
                    RequestMeta.newBuilder()
                            .setRequestId("stuck")
                            .setTraceId("trace-stuck")
                            .build(),
                    0,
                    Optional.empty());
            operations.submit(operation, () -> {
                started.countDown();
                while (!release.get()) {
                    Thread.interrupted();
                    java.util.concurrent.locks.LockSupport.parkNanos(
                            TimeUnit.MILLISECONDS.toNanos(5));
                }
            });
            assertTrue(started.await(2, TimeUnit.SECONDS));

            runtime.close();

            assertEquals(0, connectionCloseCalls.get());
            assertTrue(diagnostics.toString(StandardCharsets.UTF_8)
                    .contains("code=database.workers_not_quiesced"));
        } finally {
            release.set(true);
            operations.close(Duration.ofSeconds(2));
            runtime.close();
            writer.close();
        }
    }

    private static <T> T field(Object owner, String name, Class<T> type) throws Exception {
        Field field = owner.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return type.cast(field.get(owner));
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

package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.sql.SQLException;
import java.sql.Statement;
import java.time.Duration;
import java.util.Optional;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.junit.jupiter.api.Test;

class OperationRegistryTest {

    @Test
    void cancellationFailureConvergesBeforeTerminalAndBreaksTheSession() throws Exception {
        CountDownLatch cancelStarted = new CountDownLatch(1);
        CountDownLatch releaseCancel = new CountDownLatch(1);
        Statement statement = (Statement) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Statement.class},
                (proxy, method, arguments) -> {
                    if (method.getName().equals("cancel")) {
                        cancelStarted.countDown();
                        releaseCancel.await();
                        throw new SQLException("late cancellation failure");
                    }
                    return defaultValue(method.getReturnType());
                });
        ExecutorService waiter = Executors.newSingleThreadExecutor();
        try (OperationRegistry registry = new OperationRegistry()) {
            JdbcSession session = session("cancel-session");
            OperationRegistry.QueryOperation operation = registry.register(
                    session, meta("cancel-query"), 0, Optional.empty());
            operation.installStatement(statement);
            assertEquals(
                    CancelDisposition.CANCEL_DISPOSITION_ACCEPTED,
                    registry.cancel("cancel-query"));
            assertTrue(cancelStarted.await(2, TimeUnit.SECONDS));

            Future<RuntimeFailure> terminal = waiter.submit(() -> {
                try {
                    operation.sealAndAwaitCancellation(Duration.ofSeconds(2));
                    throw new AssertionError("cancellation failure must be terminal");
                } catch (RuntimeFailure failure) {
                    return failure;
                }
            });
            TimeUnit.MILLISECONDS.sleep(50);
            assertFalse(terminal.isDone());
            releaseCancel.countDown();

            RuntimeFailure failure = terminal.get(2, TimeUnit.SECONDS);
            assertEquals("database.cancel_failed", failure.code());
            assertEquals(JdbcSession.State.BROKEN, session.state());
            registry.finish(operation);
        } finally {
            releaseCancel.countDown();
            waiter.shutdownNow();
            waiter.awaitTermination(5, TimeUnit.SECONDS);
        }
    }

    @Test
    void shutdownReportsAWorkerThatIgnoresInterruptionAsNotQuiesced() throws Exception {
        CountDownLatch started = new CountDownLatch(1);
        AtomicBoolean release = new AtomicBoolean();
        OperationRegistry registry = new OperationRegistry();
        try {
            OperationRegistry.QueryOperation operation = registry.register(
                    session("stuck-session"), meta("stuck-query"), 0, Optional.empty());
            registry.submit(operation, () -> {
                started.countDown();
                while (!release.get()) {
                    Thread.interrupted();
                    java.util.concurrent.locks.LockSupport.parkNanos(TimeUnit.MILLISECONDS.toNanos(5));
                }
            });
            assertTrue(started.await(2, TimeUnit.SECONDS));
            assertFalse(registry.close(Duration.ofMillis(100)));
        } finally {
            release.set(true);
            registry.close(Duration.ofSeconds(2));
        }
    }

    @Test
    void reservedBatchCreditStillCountsAgainstOutstandingWindow() throws Exception {
        try (OperationRegistry registry = new OperationRegistry()) {
            OperationRegistry.QueryOperation operation = registry.register(
                    session("credit-session"),
                    meta("credit-query"),
                    ProtocolLimits.MAX_CREDIT_GRANT,
                    Optional.empty());
            for (int index = ProtocolLimits.MAX_CREDIT_GRANT;
                    index < ProtocolLimits.MAX_OUTSTANDING_CREDITS;
                    index += ProtocolLimits.MAX_CREDIT_GRANT) {
                registry.grantCredits("credit-query", ProtocolLimits.MAX_CREDIT_GRANT);
            }
            operation.awaitCredit();
            RuntimeFailure overflow = assertThrows(
                    RuntimeFailure.class,
                    () -> registry.grantCredits("credit-query", 1));
            assertEquals("operation.credit_overflow", overflow.code());
            operation.returnCredit();
            operation.sealAndAwaitCancellation(Duration.ofSeconds(1));
            registry.finish(operation);
        }
    }

    private static JdbcSession session(String id) {
        Connection connection = (Connection) Proxy.newProxyInstance(
                OperationRegistryTest.class.getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> defaultValue(method.getReturnType()));
        return new JdbcSession(
                id,
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
    }

    private static RequestMeta meta(String requestId) {
        return RequestMeta.newBuilder()
                .setRequestId(requestId)
                .setTraceId("trace-" + requestId)
                .build();
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

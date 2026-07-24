package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.JdbcColumn;
import ai.chat2db.rust.compat.protocol.v1.JdbcRow;
import ai.chat2db.rust.compat.protocol.v1.JdbcValue;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.RowBatch;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.StringReader;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Types;
import java.time.Duration;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

class JdbcStreamingTest {

    @Test
    void zeroCreditDoesNotAdvanceOrReadTheResultSet() throws Exception {
        AtomicInteger nextCalls = new AtomicInteger();
        AtomicInteger valueReads = new AtomicInteger();
        ResultSet resultSet = (ResultSet) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {ResultSet.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "next" -> nextCalls.incrementAndGet() == 1;
                    case "getCharacterStream" -> {
                        valueReads.incrementAndGet();
                        yield new StringReader("x");
                    }
                    case "isLast" -> true;
                    default -> defaultValue(method.getReturnType());
                });
        Connection connection = (Connection) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> defaultValue(method.getReturnType()));
        JdbcSession session = new JdbcSession(
                "session",
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        RequestMeta meta = RequestMeta.newBuilder()
                .setRequestId("query")
                .setTraceId("trace-query")
                .build();
        JdbcColumn column = JdbcColumn.newBuilder()
                .setOrdinal(1)
                .setJdbcType(Types.VARCHAR)
                .setJdbcTypeName("VARCHAR")
                .build();
        ExecutorService executor = Executors.newSingleThreadExecutor();

        try (OperationRegistry registry = new OperationRegistry();
                ProtocolWriter writer = new ProtocolWriter(new ByteArrayOutputStream());
                JdbcRuntime runtime = new JdbcRuntime(writer, System.err)) {
            OperationRegistry.QueryOperation operation =
                    registry.register(session, meta, 0, Optional.empty());
            JdbcRuntime.QueryProgress progress = new JdbcRuntime.QueryProgress();
            progress.advance();
            Future<JdbcRuntime.QueryCompletion> streaming = executor.submit(() -> runtime.streamRows(
                    operation,
                    resultSet,
                    List.of(column),
                    new JdbcRuntime.QueryLimits(1, 1024, 0, 0, 1024),
                    progress));

            TimeUnit.MILLISECONDS.sleep(100);
            assertFalse(streaming.isDone());
            assertEquals(0, nextCalls.get());
            assertEquals(0, valueReads.get());

            registry.grantCredits("query", 1);
            JdbcRuntime.QueryCompletion completion = streaming.get(5, TimeUnit.SECONDS);
            assertEquals(1, completion.rowCount());
            assertEquals(1, nextCalls.get());
            assertEquals(1, valueReads.get());
            operation.sealAndAwaitCancellation(Duration.ofSeconds(1));
            registry.finish(operation);
        } finally {
            executor.shutdownNow();
            executor.awaitTermination(5, TimeUnit.SECONDS);
        }
    }

    @Test
    void byteCarryReadsAtMostOneBatchWindowAndPausesUntilTheNextCredit()
            throws Exception {
        String cell = "x".repeat(24);
        JdbcRow encodedRow = JdbcRow.newBuilder()
                .addValues(JdbcValue.newBuilder().setTextValue(cell))
                .build();
        int oneRowBytes = RowBatch.newBuilder()
                .addRows(encodedRow)
                .build()
                .getSerializedSize();
        int twoRowBytes = RowBatch.newBuilder()
                .addRows(encodedRow)
                .addRows(encodedRow)
                .build()
                .getSerializedSize();
        int batchBytes = twoRowBytes - 1;
        assertTrue(encodedRow.getSerializedSize() <= batchBytes);
        assertTrue(oneRowBytes <= batchBytes);

        AtomicInteger rowIndex = new AtomicInteger(-1);
        AtomicInteger nextCalls = new AtomicInteger();
        AtomicInteger valueReads = new AtomicInteger();
        ResultSet resultSet = (ResultSet) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {ResultSet.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "next" -> {
                        nextCalls.incrementAndGet();
                        yield rowIndex.incrementAndGet() < 4;
                    }
                    case "getCharacterStream" -> {
                        valueReads.incrementAndGet();
                        yield new StringReader(cell);
                    }
                    case "isLast" -> rowIndex.get() == 3;
                    default -> defaultValue(method.getReturnType());
                });
        Connection connection = (Connection) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> defaultValue(method.getReturnType()));
        JdbcSession session = new JdbcSession(
                "carry-session",
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        RequestMeta meta = RequestMeta.newBuilder()
                .setRequestId("carry-query")
                .setTraceId("trace-carry-query")
                .build();
        JdbcColumn column = JdbcColumn.newBuilder()
                .setOrdinal(1)
                .setJdbcType(Types.VARCHAR)
                .setJdbcTypeName("VARCHAR")
                .build();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        ExecutorService executor = Executors.newSingleThreadExecutor();

        try (OperationRegistry registry = new OperationRegistry();
                ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime runtime = new JdbcRuntime(writer, System.err)) {
            OperationRegistry.QueryOperation operation =
                    registry.register(session, meta, 1, Optional.empty());
            JdbcRuntime.QueryProgress progress = new JdbcRuntime.QueryProgress();
            progress.advance();
            Future<JdbcRuntime.QueryCompletion> streaming = executor.submit(() -> runtime.streamRows(
                    operation,
                    resultSet,
                    List.of(column),
                    new JdbcRuntime.QueryLimits(2, batchBytes, 1, 0, 4096),
                    progress));

            long firstBatchDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
            while (output.size() == 0 && System.nanoTime() < firstBatchDeadline) {
                TimeUnit.MILLISECONDS.sleep(5);
            }
            assertTrue(output.size() > 0);
            assertFalse(streaming.isDone());
            assertEquals(2, nextCalls.get());
            assertEquals(2, valueReads.get());

            TimeUnit.MILLISECONDS.sleep(100);
            assertFalse(streaming.isDone());
            assertEquals(2, nextCalls.get());
            assertEquals(2, valueReads.get());

            ServerEnvelope firstBatch = FrameCodec.readFrame(
                            new ByteArrayInputStream(output.toByteArray()), ServerEnvelope.parser())
                    .orElseThrow();
            assertEquals(1, firstBatch.getRowBatch().getRowsCount());
            assertTrue(firstBatch.getRowBatch().getSerializedSize() <= batchBytes);

            registry.grantCredits("carry-query", 3);
            JdbcRuntime.QueryCompletion completion = streaming.get(5, TimeUnit.SECONDS);
            assertEquals(4, completion.rowCount());
            assertEquals(5, nextCalls.get());
            assertEquals(4, valueReads.get());
            operation.sealAndAwaitCancellation(Duration.ofSeconds(1));
            registry.finish(operation);
        } finally {
            executor.shutdownNow();
            executor.awaitTermination(5, TimeUnit.SECONDS);
        }
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

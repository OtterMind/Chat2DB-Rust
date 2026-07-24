package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;

import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.ExecuteQueryRequest;
import ai.chat2db.rust.compat.protocol.v1.ExecuteUpdateRequest;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

class JdbcResourceCloseTest {

    @Test
    void uncheckedQueryStatementCloseStillProducesATerminalFailure() throws Exception {
        AtomicInteger resultSetCloseCalls = new AtomicInteger();
        AtomicInteger statementCloseCalls = new AtomicInteger();
        ResultSetMetaData metadata = (ResultSetMetaData) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {ResultSetMetaData.class},
                (proxy, method, arguments) -> method.getName().equals("getColumnCount")
                        ? 0
                        : defaultValue(method.getReturnType()));
        ResultSet resultSet = (ResultSet) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {ResultSet.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getMetaData" -> metadata;
                    case "next" -> false;
                    case "close" -> {
                        resultSetCloseCalls.incrementAndGet();
                        yield null;
                    }
                    default -> defaultValue(method.getReturnType());
                });
        PreparedStatement statement = (PreparedStatement) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {PreparedStatement.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "executeQuery" -> resultSet;
                    case "close" -> {
                        statementCloseCalls.incrementAndGet();
                        throw new IllegalStateException("unchecked query close failure");
                    }
                    default -> defaultValue(method.getReturnType());
                });
        Connection connection = connection(statement);
        JdbcSession session = session(connection);
        RequestMeta meta = meta("query-close");
        ByteArrayOutputStream output = new ByteArrayOutputStream();

        try (ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime runtime = new JdbcRuntime(writer, System.err)) {
            OperationRegistry operations = operations(runtime);
            OperationRegistry.QueryOperation operation =
                    operations.register(session, meta, 1, Optional.empty());
            runtime.runQuery(
                    operation,
                    ExecuteQueryRequest.newBuilder().setSql("SELECT 1").build(),
                    new JdbcRuntime.QueryLimits(1, 1024, 1, 0, 1024));
        }

        List<ServerEnvelope> responses = responses(output);
        assertEquals(2, responses.size());
        assertEquals(ServerEnvelope.PayloadCase.QUERY_STARTED, responses.get(0).getPayloadCase());
        ServerEnvelope terminal = responses.get(1);
        assertEquals(ServerEnvelope.PayloadCase.ERROR, terminal.getPayloadCase());
        assertEquals("database.statement_close_failed", terminal.getError().getCode());
        assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, terminal.getError().getOutcome());
        assertEquals(SessionState.SESSION_STATE_BROKEN, terminal.getError().getSessionState());
        assertEquals(1, terminal.getMeta().getSequence());
        assertEquals(1, resultSetCloseCalls.get());
        assertEquals(1, statementCloseCalls.get());
    }

    @Test
    void uncheckedAttemptedUpdateStatementCloseIsUnknownAndBreaksSession() throws Exception {
        AtomicInteger statementCloseCalls = new AtomicInteger();
        PreparedStatement statement = (PreparedStatement) Proxy.newProxyInstance(
                getClass().getClassLoader(),
                new Class<?>[] {PreparedStatement.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "executeLargeUpdate" -> 1L;
                    case "close" -> {
                        statementCloseCalls.incrementAndGet();
                        throw new IllegalStateException("unchecked update close failure");
                    }
                    default -> defaultValue(method.getReturnType());
                });
        JdbcSession session = session(connection(statement));
        RequestMeta meta = meta("update-close");
        ByteArrayOutputStream output = new ByteArrayOutputStream();

        try (ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime runtime = new JdbcRuntime(writer, System.err)) {
            OperationRegistry.QueryOperation operation =
                    operations(runtime).register(session, meta, 0, Optional.empty());
            runtime.runUpdate(
                    operation,
                    ExecuteUpdateRequest.newBuilder()
                            .setSql("UPDATE test SET value = 1")
                            .build());
        }

        ServerEnvelope terminal = responses(output).get(0);
        assertEquals(ServerEnvelope.PayloadCase.ERROR, terminal.getPayloadCase());
        assertEquals("database.statement_close_failed", terminal.getError().getCode());
        assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, terminal.getError().getOutcome());
        assertEquals(SessionState.SESSION_STATE_BROKEN, terminal.getError().getSessionState());
        assertEquals(1, statementCloseCalls.get());
    }

    private static OperationRegistry operations(JdbcRuntime runtime) throws Exception {
        Field field = JdbcRuntime.class.getDeclaredField("operations");
        field.setAccessible(true);
        return (OperationRegistry) field.get(runtime);
    }

    private static Connection connection(PreparedStatement statement) {
        return (Connection) Proxy.newProxyInstance(
                JdbcResourceCloseTest.class.getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> method.getName().equals("prepareStatement")
                        ? statement
                        : defaultValue(method.getReturnType()));
    }

    private static JdbcSession session(Connection connection) {
        return new JdbcSession(
                "session",
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

    private static List<ServerEnvelope> responses(ByteArrayOutputStream output) throws Exception {
        ByteArrayInputStream input = new ByteArrayInputStream(output.toByteArray());
        List<ServerEnvelope> responses = new ArrayList<>();
        while (true) {
            var response = FrameCodec.readFrame(input, ServerEnvelope.parser());
            if (response.isEmpty()) {
                return responses;
            }
            responses.add(response.orElseThrow());
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

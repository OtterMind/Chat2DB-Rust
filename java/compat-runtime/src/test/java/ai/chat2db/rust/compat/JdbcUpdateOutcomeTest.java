package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;

import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
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
import java.util.Optional;
import org.junit.jupiter.api.Test;

class JdbcUpdateOutcomeTest {

    @Test
    void negativeDriverUpdateCountHasUnknownOutcomeAndBreaksSession() throws Exception {
        ServerEnvelope response = executeAttemptedUpdate(() -> -1L);
        assertEquals("database.invalid_update_count", response.getError().getCode());
        assertUnknownAndBroken(response);
    }

    @Test
    void runtimeFailureAfterExecuteAttemptHasUnknownOutcomeAndBreaksSession() throws Exception {
        ServerEnvelope response = executeAttemptedUpdate(() -> {
            throw new IllegalStateException("driver runtime failure");
        });
        assertEquals("database.update_internal_failure", response.getError().getCode());
        assertUnknownAndBroken(response);
    }

    private static ServerEnvelope executeAttemptedUpdate(UpdateExecution execution) throws Exception {
        PreparedStatement statement = (PreparedStatement) Proxy.newProxyInstance(
                JdbcUpdateOutcomeTest.class.getClassLoader(),
                new Class<?>[] {PreparedStatement.class},
                (proxy, method, arguments) -> {
                    if (method.getName().equals("executeLargeUpdate")) {
                        return execution.execute();
                    }
                    return defaultValue(method.getReturnType());
                });
        Connection connection = (Connection) Proxy.newProxyInstance(
                JdbcUpdateOutcomeTest.class.getClassLoader(),
                new Class<?>[] {Connection.class},
                (proxy, method, arguments) -> method.getName().equals("prepareStatement")
                        ? statement
                        : defaultValue(method.getReturnType()));
        JdbcSession session = new JdbcSession(
                "session",
                connection,
                null,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
        RequestMeta meta = RequestMeta.newBuilder()
                .setRequestId("update")
                .setTraceId("trace-update")
                .build();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        try (ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime runtime = new JdbcRuntime(writer, System.err)) {
            Field operationsField = JdbcRuntime.class.getDeclaredField("operations");
            operationsField.setAccessible(true);
            OperationRegistry operations = (OperationRegistry) operationsField.get(runtime);
            OperationRegistry.QueryOperation operation =
                    operations.register(session, meta, 0, Optional.empty());
            runtime.runUpdate(
                    operation,
                    ExecuteUpdateRequest.newBuilder().setSql("UPDATE test SET value = 1").build());
        }
        return FrameCodec.readFrame(
                        new ByteArrayInputStream(output.toByteArray()), ServerEnvelope.parser())
                .orElseThrow();
    }

    private static void assertUnknownAndBroken(ServerEnvelope response) {
        assertEquals(ServerEnvelope.PayloadCase.ERROR, response.getPayloadCase());
        assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, response.getError().getOutcome());
        assertEquals(SessionState.SESSION_STATE_BROKEN, response.getError().getSessionState());
    }

    @FunctionalInterface
    private interface UpdateExecution {
        long execute();
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

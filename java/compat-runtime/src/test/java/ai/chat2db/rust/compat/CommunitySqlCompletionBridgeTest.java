package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionCandidateCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHintCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHintItemCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionSnippetSlotCountLimit;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.sql.SQLException;
import org.junit.jupiter.api.Test;

class CommunitySqlCompletionBridgeTest {

    @Test
    void datasourceScopeMustFitAPositiveJavaLong() {
        assertDoesNotThrow(() -> CommunitySqlCompletionBridge.requireDatasourceScope(1));
        assertDoesNotThrow(
                () -> CommunitySqlCompletionBridge.requireDatasourceScope(Long.MAX_VALUE));

        RuntimeFailure zero = assertThrows(
                RuntimeFailure.class,
                () -> CommunitySqlCompletionBridge.requireDatasourceScope(0));
        assertEquals("community.sql_completion_datasource_scope_invalid", zero.code());
        RuntimeFailure unsignedOverflow = assertThrows(
                RuntimeFailure.class,
                () -> CommunitySqlCompletionBridge.requireDatasourceScope(-1));
        assertEquals(
                "community.sql_completion_datasource_scope_invalid", unsignedOverflow.code());
    }

    @Test
    void cacheCleanupMatchesOnlyExactCompletionScopePrefixes() {
        assertTrue(CommunitySqlCompletionBridge.belongsToDatasourceScope(
                "databases_datasourceId_7_schemaName_APP_tables", 7));
        assertTrue(CommunitySqlCompletionBridge.belongsToDatasourceScope(
                "console_parser_databases_datasourceId_7_consoleId_7", 7));
        assertFalse(CommunitySqlCompletionBridge.belongsToDatasourceScope(
                "databases_datasourceId_70_schemaName_APP_tables", 7));
        assertFalse(CommunitySqlCompletionBridge.belongsToDatasourceScope(
                "custom_datasourceId_7_value", 7));
    }

    @Test
    void completionCollectionsAcceptTheirBoundaryAndRejectOneMore() {
        int candidates = CommunitySqlCompletionCandidateCountLimit
                .COMMUNITY_SQL_COMPLETION_CANDIDATE_COUNT_LIMIT_MAX_CANDIDATES
                .getNumber();
        int hints = CommunitySqlCompletionEditorHintCountLimit
                .COMMUNITY_SQL_COMPLETION_EDITOR_HINT_COUNT_LIMIT_MAX_EDITOR_HINTS
                .getNumber();
        int hintItems = CommunitySqlCompletionEditorHintItemCountLimit
                .COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEM_COUNT_LIMIT_MAX_EDITOR_HINT_ITEMS
                .getNumber();
        int snippetSlots = CommunitySqlCompletionSnippetSlotCountLimit
                .COMMUNITY_SQL_COMPLETION_SNIPPET_SLOT_COUNT_LIMIT_MAX_SNIPPET_SLOTS
                .getNumber();

        assertDoesNotThrow(() -> CommunitySqlCompletionBridge.requireCandidateCount(candidates));
        assertDoesNotThrow(() -> CommunitySqlCompletionBridge.requireEditorHintCount(hints));
        assertDoesNotThrow(
                () -> CommunitySqlCompletionBridge.requireEditorHintItemCount(hintItems));
        assertDoesNotThrow(
                () -> CommunitySqlCompletionBridge.requireSnippetSlotCount(snippetSlots));

        assertLimit(() -> CommunitySqlCompletionBridge.requireCandidateCount(candidates + 1));
        assertLimit(() -> CommunitySqlCompletionBridge.requireEditorHintCount(hints + 1));
        assertLimit(
                () -> CommunitySqlCompletionBridge.requireEditorHintItemCount(hintItems + 1));
        assertLimit(
                () -> CommunitySqlCompletionBridge.requireSnippetSlotCount(snippetSlots + 1));
    }

    @Test
    void connectionOwnershipFailureIsAvailableToFinallyPaths() {
        Connection closed = connection((proxy, method, arguments) -> switch (method.getName()) {
            case "isClosed" -> true;
            default -> defaultValue(method.getReturnType());
        });
        RuntimeFailure closedFailure =
                CommunitySqlCompletionBridge.connectionOwnershipFailure(closed);
        assertEquals("community.sql_completion_connection_closed", closedFailure.code());

        Connection unverifiable = connection((proxy, method, arguments) -> {
            if (method.getName().equals("isClosed")) {
                throw new SQLException("state unavailable");
            }
            return defaultValue(method.getReturnType());
        });
        RuntimeFailure stateFailure =
                CommunitySqlCompletionBridge.connectionOwnershipFailure(unverifiable);
        assertEquals(
                "community.sql_completion_connection_state_failed", stateFailure.code());
    }

    private static Connection connection(InvocationHandler handler) {
        return (Connection) Proxy.newProxyInstance(
                CommunitySqlCompletionBridgeTest.class.getClassLoader(),
                new Class<?>[] {Connection.class},
                handler);
    }

    private static Object defaultValue(Class<?> type) {
        if (!type.isPrimitive()) {
            return null;
        }
        if (type == boolean.class) {
            return false;
        }
        if (type == char.class) {
            return '\0';
        }
        return 0;
    }

    private static void assertLimit(ThrowingOperation operation) {
        RuntimeFailure failure = assertThrows(RuntimeFailure.class, operation::run);
        assertEquals("protocol.limit_exceeded", failure.code());
    }

    @FunctionalInterface
    private interface ThrowingOperation {
        void run() throws RuntimeFailure;
    }
}

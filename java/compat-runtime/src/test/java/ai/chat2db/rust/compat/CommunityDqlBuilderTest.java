package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityTablePreviewSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltTablePreviewSql;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.function.Executable;

class CommunityDqlBuilderTest {

    private final CommunityDqlBuilder builder =
            new CommunityDqlBuilder(getClass().getClassLoader());

    @Test
    void quotesTheQualifiedIdentifierBeforeBuildingAndLimitingTheSelect()
            throws Exception {
        RecordingDialect dialect = new RecordingDialect();

        CommunityBuiltTablePreviewSql built = builder.build(
                dialect,
                request("MYSQL", "inventory", "reporting", "order", 200));

        assertArrayEquals(
                new String[] {"inventory", "reporting", "order"},
                dialect.identifiers);
        assertEquals(
                List.of("quote", "select", "limit"),
                dialect.calls,
                "the plugin identifier processor must run before the DQL builder");
        assertEquals(
                List.of(new SelectArguments("", "", "`inventory`.`reporting`.`order`")),
                dialect.selectArguments);
        assertEquals(
                "SELECT * FROM `inventory`.`reporting`.`order`\nLIMIT 200",
                built.getSql());
        assertEquals(200, built.getRowLimit());
    }

    @Test
    void omitsEmptyOptionalIdentifierSegments() throws Exception {
        RecordingDialect dialect = new RecordingDialect();

        CommunityBuiltTablePreviewSql built =
                builder.build(dialect, request("H2", "", "APP", "items", 1));

        assertArrayEquals(new String[] {"APP", "items"}, dialect.identifiers);
        assertEquals("SELECT * FROM `APP`.`items`\nLIMIT 1", built.getSql());
    }

    @Test
    void retriesWithRawSegmentsWhenTheBuilderQuotesTheQualifiedIdentifierAgain()
            throws Exception {
        RecordingDialect dialect = new RecordingDialect();
        dialect.quoteTableArgument = true;

        CommunityBuiltTablePreviewSql built = builder.build(
                dialect,
                request("MYSQL", "inventory", "", "order", 200));

        assertEquals(List.of("quote", "select", "select", "limit"), dialect.calls);
        assertEquals(
                List.of(
                        new SelectArguments("", "", "`inventory`.`order`"),
                        new SelectArguments("inventory", "", "order")),
                dialect.selectArguments);
        assertEquals("SELECT * FROM `inventory`.`order`\nLIMIT 200", built.getSql());
    }

    @Test
    void rejectsBuildersThatCannotPreserveTheirQuotedQualifiedIdentifier()
            throws Exception {
        RecordingDialect dialect = new RecordingDialect();
        dialect.ignoreSelectArguments = true;

        RuntimeFailure failure = assertFailure(() -> builder.build(
                dialect,
                request("MYSQL", "inventory", "", "order", 200)));

        assertEquals("community.dql_builder_incompatible", failure.code());
        assertEquals(List.of("quote", "select", "select"), dialect.calls);
    }

    @Test
    void rejectsOutOfRangeLimitsBeforeCallingThePlugin() {
        for (int rowLimit : new int[] {0, 1001}) {
            RuntimeFailure failure = assertFailure(
                    () -> builder.build(
                            new RecordingDialect(),
                            request("MYSQL", "inventory", "", "items", rowLimit)));
            assertEquals("community.dql_row_limit_invalid", failure.code());
        }
    }

    @Test
    void rejectsUnsafeAndOversizedIdentifiersBeforeCallingThePlugin() {
        for (String tableName : List.of(
                "items.detail",
                "items; DROP TABLE audit_log",
                "items--comment",
                "items/*comment*/",
                "`items`",
                " items")) {
            RuntimeFailure failure = assertFailure(
                    () -> builder.build(
                            new RecordingDialect(),
                            request("MYSQL", "inventory", "", tableName, 200)));
            assertEquals("community.dql_identifier_invalid", failure.code());
        }

        RuntimeFailure oversized = assertFailure(
                () -> builder.build(
                        new RecordingDialect(),
                        request("MYSQL", "inventory", "", "x".repeat(513), 200)));
        assertEquals("protocol.limit_exceeded", oversized.code());
    }

    private static BuildCommunityTablePreviewSqlRequest request(
            String databaseType,
            String databaseName,
            String schemaName,
            String tableName,
            int rowLimit) {
        return BuildCommunityTablePreviewSqlRequest.newBuilder()
                .setDatabaseType(databaseType)
                .setDatabaseName(databaseName)
                .setSchemaName(schemaName)
                .setTableName(tableName)
                .setRowLimit(rowLimit)
                .build();
    }

    private static RuntimeFailure assertFailure(Executable executable) {
        return assertThrows(RuntimeFailure.class, executable);
    }

    private record SelectArguments(
            String databaseName, String schemaName, String tableName) {}

    private static final class RecordingDialect implements CommunityDqlBuilder.Dialect {
        private final List<String> calls = new ArrayList<>();
        private final List<SelectArguments> selectArguments = new ArrayList<>();
        private String[] identifiers;
        private boolean quoteTableArgument;
        private boolean ignoreSelectArguments;

        @Override
        public String quoteQualifiedIdentifier(String[] identifiers) {
            calls.add("quote");
            this.identifiers = identifiers.clone();
            return "`" + String.join("`.`", identifiers) + "`";
        }

        @Override
        public String buildSelectTable(
                String databaseName, String schemaName, String tableName) {
            calls.add("select");
            selectArguments.add(new SelectArguments(databaseName, schemaName, tableName));
            if (ignoreSelectArguments) {
                return "SELECT * FROM incompatible_table";
            }
            if (quoteTableArgument) {
                return "SELECT * FROM "
                        + quote(databaseName, schemaName, tableName);
            }
            return "SELECT * FROM " + tableName;
        }

        @Override
        public String buildPageLimit(String sql, int rowLimit) {
            calls.add("limit");
            return sql + "\nLIMIT " + rowLimit;
        }

        private static String quote(String... identifiers) {
            List<String> present = new ArrayList<>();
            for (String identifier : identifiers) {
                if (!identifier.isEmpty()) {
                    present.add("`" + identifier.replace("`", "``") + "`");
                }
            }
            return String.join(".", present);
        }
    }
}

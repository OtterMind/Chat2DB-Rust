package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityDmlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlAssignment;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlColumn;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlMultiInsert;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlNull;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlRow;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlSingleInsert;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTarget;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTemporal;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTemporalKind;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlUpdate;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlValue;
import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.function.BiFunction;
import java.util.function.UnaryOperator;
import org.junit.jupiter.api.Test;

class CommunityDmlBuilderTest {

    private final CommunityDmlBuilder builder =
            new CommunityDmlBuilder(getClass().getClassLoader());

    @Test
    void buildsSingleAndMultiInsertInStableCommunityOrder() throws Exception {
        var single = request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column("id", "BIGINT"))
                .addColumns(column("label", "VARCHAR"))
                .setRow(row(decimal("7.00"), string("O'Brien"))));

        assertEquals(
                "INSERT INTO APP.items (id,label)  VALUES ('7','O''Brien')",
                builder.build(new FakeDialect(), single).getSql());

        var multi = request(CommunityDmlMultiInsert.newBuilder()
                .addColumns(column("id", "BIGINT"))
                .addColumns(column("label", "VARCHAR"))
                .addRows(row(decimal("1"), string("first")))
                .addRows(row(decimal("2"), string("second"))));
        assertEquals(
                "INSERT INTO APP.items (id,label)  VALUES ('1','first'),\n('2','second')",
                builder.build(new FakeDialect(), multi).getSql());
    }

    @Test
    void buildsUpdateWithStableAssignmentAndPredicateOrder() throws Exception {
        var request = request(CommunityDmlUpdate.newBuilder()
                .addAssignments(assignment("label", "VARCHAR", string("next")))
                .addAssignments(assignment("active", "BOOLEAN", bool(false)))
                .addPredicates(assignment("id", "BIGINT", decimal("7")))
                .addPredicates(assignment("tenant_id", "BIGINT", decimal("3"))));

        assertEquals(
                "UPDATE APP.items SET label = 'next',active = FALSE"
                        + " WHERE id = '7' AND tenant_id = '3'",
                builder.build(new FakeDialect(), request).getSql());
    }

    @Test
    void normalizesTypedValuesBeforeTheDialectSeesThem() throws Exception {
        var request = request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column("amount", "DECIMAL"))
                .addColumns(column("active", "BOOLEAN"))
                .addColumns(column("created_at", "TIMESTAMP"))
                .addColumns(column("payload", "BINARY"))
                .addColumns(column("note", "VARCHAR"))
                .setRow(row(
                        decimal("007.5000"),
                        bool(true),
                        temporal(
                                CommunityDmlTemporalKind
                                        .COMMUNITY_DML_TEMPORAL_KIND_LOCAL_DATETIME,
                                "2026-07-27T12:34:56"),
                        binary(0x00, 0xff),
                        nullValue())));

        assertEquals(
                "INSERT INTO APP.items (amount,active,created_at,payload,note)  VALUES "
                        + "(7.5,TRUE,'2026-07-27 12:34:56',0x00FF,NULL)",
                builder.build(new FakeDialect(), request).getSql());
    }

    @Test
    void rejectsMysqlFunctionTokensWhenTheProtocolValueIsAString() {
        FakeDialect mysqlLike = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value != null
                                && (value.equalsIgnoreCase("now()")
                                        || value.equalsIgnoreCase("default"))
                        ? value
                        : FakeDialect.defaultRender(column, value));

        for (String token : List.of("now()", "DEFAULT")) {
            RuntimeFailure failure = assertFailure(
                    () -> builder.build(mysqlLike, single(column("value", "VARCHAR"), string(token))));
            assertEquals("community.dml_value_not_supported", failure.code());
            assertFalse(failure.getMessage().contains(token));
        }
    }

    @Test
    void rejectsMysqlBackslashCrossColumnStringInjection() {
        String payload = "); DROP TABLE audit_log; -- ";
        FakeDialect mysqlTimestampLike = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : "'" + value + "'");
        BuildCommunityDmlRequest request = request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column("a", "TIMESTAMP"))
                .addColumns(column("b", "TIMESTAMP"))
                .setRow(row(string("\\"), string(payload))));

        RuntimeFailure failure =
                assertFailure(() -> builder.build(mysqlTimestampLike, request));
        assertEquals("community.dml_value_not_supported", failure.code());
        assertFalse(failure.getMessage().contains(payload));
    }

    @Test
    void normalizesBooleanProcessorInputForNumericBooleanDialects() throws Exception {
        FakeDialect quotedBoolean = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : FakeDialect.quote(value));
        assertTrue(builder
                .build(
                        quotedBoolean,
                        request(
                                "MYSQL",
                                CommunityDmlSingleInsert.newBuilder()
                                        .addColumns(column("active", "BOOLEAN"))
                                        .setRow(row(bool(true)))))
                .getSql()
                .contains("'1'"));

        FakeDialect rawBoolean = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : value);
        assertTrue(builder
                .build(
                        rawBoolean,
                        request(
                                "SQLSERVER",
                                CommunityDmlSingleInsert.newBuilder()
                                        .addColumns(column("active", "BIT"))
                                        .setRow(row(bool(true)))))
                .getSql()
                .contains("(1)"));

        FakeDialect mysqlBit = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null
                        ? "NULL"
                        : "b'"
                                + (value.equals("true") || value.equals("1") ? "1" : "0")
                                + "'");
        assertTrue(builder
                .build(
                        mysqlBit,
                        request(
                                "MYSQL",
                                CommunityDmlSingleInsert.newBuilder()
                                        .addColumns(column("active", "BIT"))
                                        .setRow(row(bool(true)))))
                .getSql()
                .contains("b'1'"));
    }

    @Test
    void normalizesQuotedH2BooleanLiteralsAcrossInsertAndUpdate() throws Exception {
        FakeDialect quotedBoolean = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : FakeDialect.quote(value));

        assertEquals(
                "INSERT INTO APP.items (active)  VALUES (TRUE)",
                builder.build(
                                quotedBoolean,
                                request(
                                        "H2",
                                        CommunityDmlSingleInsert.newBuilder()
                                                .addColumns(column("active", "BOOLEAN"))
                                                .setRow(row(bool(true)))))
                        .getSql());

        assertEquals(
                "UPDATE APP.items SET active = FALSE WHERE enabled = TRUE",
                builder.build(
                                quotedBoolean,
                                request(CommunityDmlUpdate.newBuilder()
                                        .addAssignments(assignment(
                                                "active", "BOOLEAN", bool(false)))
                                        .addPredicates(assignment(
                                                "enabled", "BOOLEAN", bool(true)))))
                        .getSql());
    }

    @Test
    void rejectsTemporalTypeMismatchesAndAlteredExpressions() throws Exception {
        RuntimeFailure mismatch = assertFailure(() -> builder.build(
                new FakeDialect(),
                single(
                        column("created_at", "DATE"),
                        temporal(
                                CommunityDmlTemporalKind.COMMUNITY_DML_TEMPORAL_KIND_TIME,
                                "12:34:56"))));
        assertEquals("community.dml_value_not_supported", mismatch.code());

        FakeDialect alteredExpression = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> "TO_DATE('" + value + "', 'YYYY-MM-DD')");
        RuntimeFailure altered = assertFailure(() -> builder.build(
                alteredExpression,
                single(
                        column("created_at", "DATE"),
                        temporal(
                                CommunityDmlTemporalKind.COMMUNITY_DML_TEMPORAL_KIND_DATE,
                                "2026-07-27"))));
        assertEquals("community.dml_value_not_supported", altered.code());

        FakeDialect oracleDate = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) ->
                        "TO_DATE('" + value + "', 'SYYYY-MM-DD HH24:MI:SS')");
        assertTrue(builder
                .build(
                        oracleDate,
                        single(
                                column("created_at", "DATE"),
                                temporal(
                                        CommunityDmlTemporalKind
                                                .COMMUNITY_DML_TEMPORAL_KIND_DATE,
                                        "2026-07-27")))
                .getSql()
                .contains("TO_DATE"));

        CommunityDmlColumn insufficientScale = CommunityDmlColumn.newBuilder()
                .setName("created_at")
                .setDataTypeName("TIMESTAMP")
                .setScale(2)
                .build();
        RuntimeFailure truncated = assertFailure(() -> builder.build(
                new FakeDialect(),
                single(
                        insufficientScale,
                        temporal(
                                CommunityDmlTemporalKind
                                        .COMMUNITY_DML_TEMPORAL_KIND_LOCAL_DATETIME,
                                "2026-07-27T12:34:56.123"))));
        assertEquals("community.dml_value_not_supported", truncated.code());
    }

    @Test
    void rejectsTypedValuesForIncompatibleColumnTypes() {
        assertEquals(
                "community.dml_value_not_supported",
                assertFailure(() -> builder.build(
                                new FakeDialect(),
                                single(column("value", "VARCHAR"), bool(true))))
                        .code());
        assertEquals(
                "community.dml_value_not_supported",
                assertFailure(() -> builder.build(
                                new FakeDialect(),
                                single(column("value", "VARCHAR"), decimal("7"))))
                        .code());
        assertEquals(
                "community.dml_value_not_supported",
                assertFailure(() -> builder.build(
                                new FakeDialect(),
                                single(column("value", "VARCHAR"), binary(0x01))))
                        .code());
    }

    @Test
    void rejectsBinaryWhenTheDialectFallsBackToAQuotedHexString() {
        FakeDialect h2Like = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : FakeDialect.quote(value));

        RuntimeFailure failure = assertFailure(
                () -> builder.build(
                        h2Like, single(column("payload", "VARBINARY"), binary(0x00, 0xff))));
        assertEquals("community.dml_value_not_supported", failure.code());
    }

    @Test
    void rejectsQuotedEmptyBinaryThatWouldBecomeOracleNull() {
        FakeDialect oracleLike = new FakeDialect(
                UnaryOperator.identity(),
                (column, value) -> value == null ? "NULL" : "'" + value.substring(2) + "'");

        RuntimeFailure failure = assertFailure(() -> builder.build(
                oracleLike, single(column("payload", "RAW"), binary())));
        assertEquals("community.dml_value_not_supported", failure.code());
    }

    @Test
    void rejectsMalformedDecimalsAndTemporalValuesWithoutEchoingThem() {
        RuntimeFailure decimalFailure = assertFailure(() -> builder.build(
                new FakeDialect(), single(column("amount", "DECIMAL"), decimal("1e309"))));
        assertEquals("community.dml_decimal_invalid", decimalFailure.code());
        assertFalse(decimalFailure.getMessage().contains("1e309"));

        RuntimeFailure leadingPlus = assertFailure(() -> builder.build(
                new FakeDialect(), single(column("amount", "DECIMAL"), decimal("+1"))));
        assertEquals("community.dml_decimal_invalid", leadingPlus.code());

        RuntimeFailure temporalFailure = assertFailure(() -> builder.build(
                new FakeDialect(),
                single(
                        column("created_at", "TIMESTAMP"),
                        temporal(
                                CommunityDmlTemporalKind.COMMUNITY_DML_TEMPORAL_KIND_DATE,
                                "2026-02-30"))));
        assertEquals("community.dml_temporal_invalid", temporalFailure.code());
        assertFalse(temporalFailure.getMessage().contains("2026-02-30"));
    }

    @Test
    void rejectsUnsafeIdentifiersBeforeAndAfterDialectQuoting() {
        List<String> unsafe = List.of(
                "a.b",
                "a'b",
                "a\"b",
                "a`b",
                "a[b",
                "a]b",
                "a;b",
                "a--b",
                "a/*b",
                "a*/b",
                "a\0b");
        for (String identifier : unsafe) {
            RuntimeFailure failure = assertFailure(() -> builder.build(
                    new FakeDialect(),
                    single(column(identifier, "VARCHAR"), string("safe"))));
            assertEquals("community.dml_identifier_invalid", failure.code());
            assertFalse(failure.getMessage().contains(identifier));
        }

        FakeDialect maliciousProcessor = new FakeDialect(
                ignored -> "safe; DROP TABLE sentinel", FakeDialect::defaultRender);
        RuntimeFailure processorFailure = assertFailure(() -> builder.build(
                maliciousProcessor, single(column("safe", "VARCHAR"), string("value"))));
        assertEquals("community.dml_identifier_invalid", processorFailure.code());
    }

    @Test
    void rejectsDuplicateColumnsAndIdentifierProcessorRewrites() {
        var duplicate = request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column("id", "BIGINT"))
                .addColumns(column("id", "BIGINT"))
                .setRow(row(decimal("1"), decimal("2"))));
        assertEquals(
                "community.dml_duplicate_column",
                assertFailure(() -> builder.build(new FakeDialect(), duplicate)).code());

        FakeDialect caseFolding = new FakeDialect(
                value -> value.toLowerCase(Locale.ROOT), FakeDialect::defaultRender);
        var collision = request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column("ID", "BIGINT"))
                .addColumns(column("id", "BIGINT"))
                .setRow(row(decimal("1"), decimal("2"))));
        assertEquals(
                "community.dml_identifier_invalid",
                assertFailure(() -> builder.build(caseFolding, collision)).code());
    }

    @Test
    void rejectsMissingAndInconsistentInsertStructure() {
        assertCode(
                "community.dml_target_required",
                BuildCommunityDmlRequest.newBuilder()
                        .setDatabaseType("H2")
                        .setSingleInsert(CommunityDmlSingleInsert.getDefaultInstance())
                        .build());
        assertCode(
                "community.dml_statement_required",
                BuildCommunityDmlRequest.newBuilder()
                        .setDatabaseType("H2")
                        .setTarget(target())
                        .build());
        assertCode(
                "community.dml_columns_required",
                request(CommunityDmlSingleInsert.newBuilder().setRow(row())));
        assertCode(
                "community.dml_row_required",
                request(CommunityDmlSingleInsert.newBuilder()
                        .addColumns(column("id", "BIGINT"))));
        assertCode(
                "community.dml_row_width_mismatch",
                request(CommunityDmlSingleInsert.newBuilder()
                        .addColumns(column("id", "BIGINT"))
                        .setRow(row())));
        assertCode(
                "community.dml_rows_required",
                request(CommunityDmlMultiInsert.newBuilder()
                        .addColumns(column("id", "BIGINT"))));
        assertCode(
                "community.dml_value_required",
                single(column("id", "BIGINT"), CommunityDmlValue.getDefaultInstance()));
    }

    @Test
    void rejectsUnsafeUpdateShapes() {
        assertCode(
                "community.dml_update_assignments_required",
                request(CommunityDmlUpdate.newBuilder()
                        .addPredicates(assignment("id", "BIGINT", decimal("1")))));
        assertCode(
                "community.dml_update_predicates_required",
                request(CommunityDmlUpdate.newBuilder()
                        .addAssignments(assignment("label", "VARCHAR", string("next")))));
        assertCode(
                "community.dml_null_predicate_not_supported",
                request(CommunityDmlUpdate.newBuilder()
                        .addAssignments(assignment("label", "VARCHAR", string("next")))
                        .addPredicates(assignment("id", "BIGINT", nullValue()))));
    }

    @Test
    void enforcesCountAndByteLimitsIncludingQuotedIdentifierExpansion() throws Exception {
        var tooManyRows = CommunityDmlMultiInsert.newBuilder()
                .addColumns(column("id", "BIGINT"));
        for (int index = 0; index < 4097; index++) {
            tooManyRows.addRows(row(decimal("1")));
        }
        assertEquals(
                "protocol.limit_exceeded",
                assertFailure(() -> builder.build(new FakeDialect(), request(tooManyRows))).code());

        var tooManyValues = CommunityDmlMultiInsert.newBuilder();
        for (int column = 0; column < 2048; column++) {
            tooManyValues.addColumns(column("c" + column, "BIGINT"));
        }
        for (int row = 0; row < 17; row++) {
            tooManyValues.addRows(CommunityDmlRow.getDefaultInstance());
        }
        assertEquals(
                "protocol.limit_exceeded",
                assertFailure(() -> builder.build(new FakeDialect(), request(tooManyValues))).code());

        FakeDialect quoting = new FakeDialect(
                value -> "\"" + value + "\"", FakeDialect::defaultRender);
        RuntimeFailure expansion = assertFailure(() -> builder.build(
                quoting,
                single(column("a".repeat(512), "VARCHAR"), string("value"))));
        assertEquals("protocol.limit_exceeded", expansion.code());

        RuntimeFailure valueLimit = assertFailure(() -> builder.build(
                new FakeDialect(),
                single(column("value", "VARCHAR"), string("x".repeat(262145)))));
        assertEquals("protocol.limit_exceeded", valueLimit.code());

        CommunityDmlColumn maximumPrecisionMinimumScale = CommunityDmlColumn.newBuilder()
                .setName("amount")
                .setDataTypeName("DECIMAL")
                .setPrecision(Integer.MAX_VALUE)
                .setScale(Integer.MIN_VALUE)
                .build();
        assertTrue(builder
                .build(
                        new FakeDialect(),
                        single(maximumPrecisionMinimumScale, decimal("7")))
                .getSql()
                .contains("7"));

        CommunityDmlColumn maximumScale = CommunityDmlColumn.newBuilder()
                .setName("amount")
                .setDataTypeName("DECIMAL")
                .setScale(Integer.MAX_VALUE)
                .build();
        assertTrue(builder
                .build(new FakeDialect(), single(maximumScale, decimal("7")))
                .getSql()
                .contains("7"));

        CommunityDmlColumn negativePrecision = CommunityDmlColumn.newBuilder()
                .setName("amount")
                .setDataTypeName("DECIMAL")
                .setPrecision(-1)
                .build();
        assertEquals(
                "community.dml_precision_invalid",
                assertFailure(() -> builder.build(
                                new FakeDialect(),
                                single(negativePrecision, decimal("7"))))
                        .code());
    }

    @Test
    void rejectsOversizedBuilderOutput() {
        FakeDialect oversized = new FakeDialect() {
            @Override
            public String buildSingleInsert(
                    CommunityDmlBuilder.Target target,
                    List<String> columns,
                    List<String> values) {
                return "x".repeat(ProtocolLimits.MAX_SQL_BYTES + 1);
            }
        };

        RuntimeFailure failure = assertFailure(() -> builder.build(
                oversized, single(column("id", "BIGINT"), decimal("1"))));
        assertEquals("protocol.limit_exceeded", failure.code());
    }

    @Test
    void mapsUnsupportedAndUnexpectedSpiFailuresAndRestoresTheContextLoader() {
        ClassLoader original = Thread.currentThread().getContextClassLoader();
        RuntimeFailure unsupported = assertFailure(() -> builder.build(
                new UnsupportedPlugin(), single(column("id", "BIGINT"), decimal("1"))));
        assertEquals("community.dml_builder_not_supported", unsupported.code());
        assertSame(original, Thread.currentThread().getContextClassLoader());

        RuntimeFailure failed = assertFailure(() -> builder.build(
                new FailingPlugin(), single(column("id", "BIGINT"), decimal("1"))));
        assertEquals("community.dml_builder_failed", failed.code());
        assertFalse(failed.getMessage().contains("sensitive-plugin-detail"));
        assertSame(original, Thread.currentThread().getContextClassLoader());
    }

    private void assertCode(String code, BuildCommunityDmlRequest request) {
        assertEquals(code, assertFailure(() -> builder.build(new FakeDialect(), request)).code());
    }

    private static RuntimeFailure assertFailure(ThrowingAction action) {
        return assertThrows(RuntimeFailure.class, action::run);
    }

    private static BuildCommunityDmlRequest request(CommunityDmlSingleInsert.Builder statement) {
        return request("H2", statement);
    }

    private static BuildCommunityDmlRequest request(
            String databaseType, CommunityDmlSingleInsert.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType(databaseType)
                .setTarget(target())
                .setSingleInsert(statement)
                .build();
    }

    private static BuildCommunityDmlRequest request(CommunityDmlMultiInsert.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(target())
                .setMultiInsert(statement)
                .build();
    }

    private static BuildCommunityDmlRequest request(CommunityDmlUpdate.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(target())
                .setUpdate(statement)
                .build();
    }

    private static BuildCommunityDmlRequest single(
            CommunityDmlColumn column, CommunityDmlValue value) {
        return request(CommunityDmlSingleInsert.newBuilder()
                .addColumns(column)
                .setRow(row(value)));
    }

    private static CommunityDmlTarget target() {
        return CommunityDmlTarget.newBuilder()
                .setSchemaName("APP")
                .setTableName("items")
                .build();
    }

    private static CommunityDmlColumn column(String name, String type) {
        return CommunityDmlColumn.newBuilder()
                .setName(name)
                .setDataTypeName(type)
                .build();
    }

    private static CommunityDmlAssignment assignment(
            String name, String type, CommunityDmlValue value) {
        return CommunityDmlAssignment.newBuilder()
                .setColumn(column(name, type))
                .setValue(value)
                .build();
    }

    private static CommunityDmlRow row(CommunityDmlValue... values) {
        return CommunityDmlRow.newBuilder().addAllValues(List.of(values)).build();
    }

    private static CommunityDmlValue string(String value) {
        return CommunityDmlValue.newBuilder().setStringValue(value).build();
    }

    private static CommunityDmlValue decimal(String value) {
        return CommunityDmlValue.newBuilder().setDecimalValue(value).build();
    }

    private static CommunityDmlValue bool(boolean value) {
        return CommunityDmlValue.newBuilder().setBooleanValue(value).build();
    }

    private static CommunityDmlValue temporal(
            CommunityDmlTemporalKind kind, String value) {
        return CommunityDmlValue.newBuilder()
                .setTemporalValue(CommunityDmlTemporal.newBuilder()
                        .setKind(kind)
                        .setIso8601(value))
                .build();
    }

    private static CommunityDmlValue binary(int... values) {
        byte[] bytes = new byte[values.length];
        for (int index = 0; index < values.length; index++) {
            bytes[index] = (byte) values[index];
        }
        return CommunityDmlValue.newBuilder()
                .setBinaryValue(ByteString.copyFrom(bytes))
                .build();
    }

    private static CommunityDmlValue nullValue() {
        return CommunityDmlValue.newBuilder()
                .setNullValue(CommunityDmlNull.getDefaultInstance())
                .build();
    }

    @FunctionalInterface
    private interface ThrowingAction {
        void run() throws Exception;
    }

    private static class FakeDialect implements CommunityDmlBuilder.Dialect {
        private final UnaryOperator<String> quoter;
        private final BiFunction<CommunityDmlBuilder.Column, String, String> renderer;

        private FakeDialect() {
            this(UnaryOperator.identity(), FakeDialect::defaultRender);
        }

        private FakeDialect(
                UnaryOperator<String> quoter,
                BiFunction<CommunityDmlBuilder.Column, String, String> renderer) {
            this.quoter = quoter;
            this.renderer = renderer;
        }

        @Override
        public String quoteIdentifier(String identifier) {
            return quoter.apply(identifier);
        }

        @Override
        public String renderValue(CommunityDmlBuilder.Column column, String value) {
            return renderer.apply(column, value);
        }

        @Override
        public String buildSingleInsert(
                CommunityDmlBuilder.Target target,
                List<String> columns,
                List<String> values) {
            return baseInsert(target, columns) + "(" + String.join(",", values) + ")";
        }

        @Override
        public String buildMultiInsert(
                CommunityDmlBuilder.Target target,
                List<String> columns,
                List<List<String>> rows) {
            List<String> rendered = new ArrayList<>(rows.size());
            for (List<String> row : rows) {
                rendered.add("(" + String.join(",", row) + ")");
            }
            return baseInsert(target, columns) + String.join(",\n", rendered);
        }

        @Override
        public String buildUpdate(
                CommunityDmlBuilder.Target target,
                Map<String, String> assignments,
                Map<String, String> predicates) {
            return "UPDATE "
                    + qualified(target)
                    + " SET "
                    + clauses(assignments, ",")
                    + " WHERE "
                    + clauses(predicates, " AND ");
        }

        private static String defaultRender(CommunityDmlBuilder.Column column, String value) {
            if (value == null) {
                return "NULL";
            }
            String type = column.dataTypeName().toUpperCase(Locale.ROOT);
            return switch (type) {
                case "DECIMAL", "NUMBER", "BOOLEAN", "BIT", "BINARY", "VARBINARY" -> value;
                default -> quote(value);
            };
        }

        private static String quote(String value) {
            return "'" + value.replace("'", "''") + "'";
        }

        private static String baseInsert(
                CommunityDmlBuilder.Target target, List<String> columns) {
            return "INSERT INTO "
                    + qualified(target)
                    + " ("
                    + String.join(",", columns)
                    + ")  VALUES ";
        }

        private static String qualified(CommunityDmlBuilder.Target target) {
            List<String> segments = new ArrayList<>();
            if (!target.databaseName().isEmpty()) {
                segments.add(target.databaseName());
            }
            if (!target.schemaName().isEmpty()) {
                segments.add(target.schemaName());
            }
            segments.add(target.tableName());
            return String.join(".", segments);
        }

        private static String clauses(Map<String, String> values, String separator) {
            return values.entrySet().stream()
                    .map(entry -> entry.getKey() + " = " + entry.getValue())
                    .reduce((left, right) -> left + separator + right)
                    .orElseThrow();
        }
    }

    public static final class UnsupportedPlugin {
        public Object getSqlBuilder() {
            return new UnsupportedBuilder();
        }

        public Object getValueProcessor() {
            return new Object();
        }

        public Object getSQLIdentifierProcessor() {
            return new Object();
        }
    }

    public static final class UnsupportedBuilder {
        public Object dml() {
            throw new UnsupportedOperationException("unsupported");
        }
    }

    public static final class FailingPlugin {
        public Object getSqlBuilder() {
            throw new IllegalStateException("sensitive-plugin-detail");
        }

        public Object getValueProcessor() {
            return new Object();
        }

        public Object getSQLIdentifierProcessor() {
            return new Object();
        }
    }
}

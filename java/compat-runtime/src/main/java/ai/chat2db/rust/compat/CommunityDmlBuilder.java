package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityDmlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltDml;
import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlAssignment;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlColumn;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlMultiInsert;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlRow;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlSingleInsert;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTarget;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlTemporal;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlUpdate;
import ai.chat2db.rust.compat.protocol.v1.CommunityDmlValue;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.math.BigDecimal;
import java.time.DateTimeException;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.time.OffsetDateTime;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;

/** Bounded reflective adapter for Community's dialect DML builders. */
final class CommunityDmlBuilder {

    private static final String DATA_TYPE_CLASS =
            "ai.chat2db.community.domain.api.model.metadata.DataType";
    private static final String SQL_DATA_VALUE_CLASS =
            "ai.chat2db.community.domain.api.model.value.SQLDataValue";
    private static final String SINGLE_INSERT_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.SingleInsertSqlRequest";
    private static final String MULTI_INSERT_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.MultiInsertSqlRequest";
    private static final String UPDATE_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.UpdateSqlRequest";
    private static final String SQL_SERVER_PLUGIN_PREFIX = "ai.chat2db.plugin.sqlserver.";
    private static final Pattern DECIMAL = Pattern.compile("-?[0-9]+(?:\\.[0-9]+)?");
    private static final char[] HEX = "0123456789ABCDEF".toCharArray();

    private static final int MAX_COLUMNS = CommunityDmlCountLimit
            .COMMUNITY_DML_COUNT_LIMIT_MAX_COLUMNS
            .getNumber();
    private static final int MAX_ROWS = CommunityDmlCountLimit
            .COMMUNITY_DML_COUNT_LIMIT_MAX_ROWS
            .getNumber();
    private static final int MAX_VALUES = CommunityDmlCountLimit
            .COMMUNITY_DML_COUNT_LIMIT_MAX_VALUES
            .getNumber();
    private static final int MAX_IDENTIFIER_BYTES = CommunityDmlByteLimit
            .COMMUNITY_DML_BYTE_LIMIT_MAX_IDENTIFIER_BYTES
            .getNumber();
    private static final int MAX_DATA_TYPE_NAME_BYTES = CommunityDmlByteLimit
            .COMMUNITY_DML_BYTE_LIMIT_MAX_DATA_TYPE_NAME_BYTES
            .getNumber();
    private static final int MAX_DECIMAL_BYTES = CommunityDmlByteLimit
            .COMMUNITY_DML_BYTE_LIMIT_MAX_DECIMAL_BYTES
            .getNumber();
    private static final int MAX_TEMPORAL_BYTES = CommunityDmlByteLimit
            .COMMUNITY_DML_BYTE_LIMIT_MAX_TEMPORAL_BYTES
            .getNumber();
    private static final int MAX_VALUE_BYTES = CommunityDmlByteLimit
            .COMMUNITY_DML_BYTE_LIMIT_MAX_VALUE_BYTES
            .getNumber();
    private static final int MAX_RESPONSE_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES.getNumber();

    private final ClassLoader loader;

    CommunityDmlBuilder(ClassLoader loader) {
        this.loader = loader;
    }

    CommunityBuiltDml build(Object plugin, BuildCommunityDmlRequest request)
            throws RuntimeFailure {
        validateRequest(request);
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            return build(new ReflectiveDialect(loader, plugin), request);
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (InvocationTargetException failure) {
            Throwable cause = invocationCause(failure);
            if (cause instanceof UnsupportedOperationException) {
                throw notSupported();
            }
            throw failed(cause);
        } catch (UnsupportedOperationException failure) {
            throw notSupported();
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw failed(failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    CommunityBuiltDml build(Dialect dialect, BuildCommunityDmlRequest request)
            throws ReflectiveOperationException, RuntimeFailure {
        validateRequest(request);
        Target target = target(dialect, request.getTarget());
        String databaseType = request.getDatabaseType();
        String sql = switch (request.getStatementCase()) {
            case SINGLE_INSERT ->
                    singleInsert(dialect, target, request.getSingleInsert(), databaseType);
            case MULTI_INSERT ->
                    multiInsert(dialect, target, request.getMultiInsert(), databaseType);
            case UPDATE -> update(dialect, target, request.getUpdate(), databaseType);
            case STATEMENT_NOT_SET -> throw RuntimeFailure.validation(
                    "community.dml_statement_required",
                    "a Community DML statement is required");
        };
        ProtocolLimits.requireNonBlankUtf8(sql, ProtocolLimits.MAX_SQL_BYTES, "built_dml_sql");
        CommunityBuiltDml response = CommunityBuiltDml.newBuilder().setSql(sql).build();
        if (response.getSerializedSize() > MAX_RESPONSE_BYTES) {
            throw RuntimeFailure.limit("Community DML response", MAX_RESPONSE_BYTES);
        }
        return response;
    }

    private static String singleInsert(
            Dialect dialect,
            Target target,
            CommunityDmlSingleInsert insert,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        List<Column> columns = columns(dialect, insert.getColumnsList(), "insert columns");
        if (!insert.hasRow()) {
            throw RuntimeFailure.validation(
                    "community.dml_row_required", "an insert row is required");
        }
        List<String> values = row(dialect, columns, insert.getRow(), false, databaseType);
        return dialect.buildSingleInsert(target, identifiers(columns), values);
    }

    private static String multiInsert(
            Dialect dialect,
            Target target,
            CommunityDmlMultiInsert insert,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        List<Column> columns = columns(dialect, insert.getColumnsList(), "insert columns");
        int rowCount = insert.getRowsCount();
        if (rowCount == 0) {
            throw RuntimeFailure.validation(
                    "community.dml_rows_required", "insert rows are required");
        }
        requireCount(rowCount, MAX_ROWS, "Community DML rows");
        requireValueCount((long) columns.size() * rowCount);
        List<List<String>> rows = new ArrayList<>(rowCount);
        for (CommunityDmlRow row : insert.getRowsList()) {
            rows.add(row(dialect, columns, row, false, databaseType));
        }
        return dialect.buildMultiInsert(target, identifiers(columns), rows);
    }

    private static String update(
            Dialect dialect,
            Target target,
            CommunityDmlUpdate update,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        if (update.getAssignmentsCount() == 0) {
            throw RuntimeFailure.validation(
                    "community.dml_update_assignments_required",
                    "update assignments are required");
        }
        if (update.getPredicatesCount() == 0) {
            throw RuntimeFailure.validation(
                    "community.dml_update_predicates_required",
                    "update predicates are required");
        }
        requireCount(update.getAssignmentsCount(), MAX_COLUMNS, "Community DML assignments");
        requireCount(update.getPredicatesCount(), MAX_COLUMNS, "Community DML predicates");
        requireValueCount((long) update.getAssignmentsCount() + update.getPredicatesCount());
        Map<String, String> assignments = assignments(
                dialect,
                update.getAssignmentsList(),
                false,
                "update assignments",
                databaseType);
        Map<String, String> predicates = assignments(
                dialect,
                update.getPredicatesList(),
                true,
                "update predicates",
                databaseType);
        return dialect.buildUpdate(target, assignments, predicates);
    }

    private static Map<String, String> assignments(
            Dialect dialect,
            List<CommunityDmlAssignment> values,
            boolean predicate,
            String field,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        Map<String, String> result = new LinkedHashMap<>();
        Set<String> rawNames = new HashSet<>();
        for (CommunityDmlAssignment assignment : values) {
            if (!assignment.hasColumn()) {
                throw RuntimeFailure.validation(
                        "community.dml_column_required", "a Community DML column is required");
            }
            if (!assignment.hasValue()) {
                throw RuntimeFailure.validation(
                        "community.dml_value_required", "a Community DML value is required");
            }
            if (predicate
                    && assignment.getValue().getValueCase()
                            == CommunityDmlValue.ValueCase.NULL_VALUE) {
                throw RuntimeFailure.validation(
                        "community.dml_null_predicate_not_supported",
                        "null update predicates are not supported");
            }
            Column column = column(dialect, assignment.getColumn());
            if (!rawNames.add(assignment.getColumn().getName())
                    || result.containsKey(column.identifier())) {
                throw RuntimeFailure.validation(
                        "community.dml_duplicate_column", field + " contain a duplicate column");
            }
            result.put(
                    column.identifier(),
                    value(dialect, column, assignment.getValue(), databaseType));
        }
        return result;
    }

    private static List<Column> columns(
            Dialect dialect, List<CommunityDmlColumn> requested, String field)
            throws ReflectiveOperationException, RuntimeFailure {
        if (requested.isEmpty()) {
            throw RuntimeFailure.validation(
                    "community.dml_columns_required", field + " are required");
        }
        requireCount(requested.size(), MAX_COLUMNS, "Community DML columns");
        List<Column> columns = new ArrayList<>(requested.size());
        Set<String> rawNames = new HashSet<>();
        Set<String> identifiers = new HashSet<>();
        for (CommunityDmlColumn requestedColumn : requested) {
            Column column = column(dialect, requestedColumn);
            if (!rawNames.add(requestedColumn.getName())
                    || !identifiers.add(column.identifier())) {
                throw RuntimeFailure.validation(
                        "community.dml_duplicate_column", field + " contain a duplicate column");
            }
            columns.add(column);
        }
        return List.copyOf(columns);
    }

    private static Column column(Dialect dialect, CommunityDmlColumn requested)
            throws ReflectiveOperationException, RuntimeFailure {
        String identifier = identifier(dialect, requested.getName(), false, "column_name");
        ProtocolLimits.requireNonBlankUtf8(
                requested.getDataTypeName(), MAX_DATA_TYPE_NAME_BYTES, "data_type_name");
        requireNoAsciiControl(requested.getDataTypeName(), "data_type_name");
        int precision = requested.hasPrecision() ? requested.getPrecision() : 0;
        if (precision < 0) {
            throw RuntimeFailure.validation(
                    "community.dml_precision_invalid",
                    "Community DML precision must be non-negative");
        }
        int scale = requested.hasScale() ? requested.getScale() : 0;
        return new Column(
                identifier, requested.getDataTypeName(), precision, scale);
    }

    private static List<String> row(
            Dialect dialect,
            List<Column> columns,
            CommunityDmlRow row,
            boolean predicate,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        if (row.getValuesCount() != columns.size()) {
            throw RuntimeFailure.validation(
                    "community.dml_row_width_mismatch",
                    "Community DML row width must match the column count");
        }
        requireValueCount(row.getValuesCount());
        List<String> values = new ArrayList<>(columns.size());
        for (int index = 0; index < columns.size(); index++) {
            CommunityDmlValue requested = row.getValues(index);
            if (predicate
                    && requested.getValueCase() == CommunityDmlValue.ValueCase.NULL_VALUE) {
                throw RuntimeFailure.validation(
                        "community.dml_null_predicate_not_supported",
                        "null update predicates are not supported");
            }
            values.add(value(dialect, columns.get(index), requested, databaseType));
        }
        return List.copyOf(values);
    }

    private static String value(
            Dialect dialect,
            Column column,
            CommunityDmlValue requested,
            String databaseType)
            throws ReflectiveOperationException, RuntimeFailure {
        String canonical;
        ValueKind kind;
        switch (requested.getValueCase()) {
            case NULL_VALUE -> {
                canonical = null;
                kind = ValueKind.NULL;
            }
            case STRING_VALUE -> {
                canonical = requested.getStringValue();
                ProtocolLimits.requireUtf8(canonical, MAX_VALUE_BYTES, "dml_string_value");
                kind = ValueKind.STRING;
            }
            case DECIMAL_VALUE -> {
                canonical = decimal(requested.getDecimalValue());
                kind = ValueKind.DECIMAL;
            }
            case BOOLEAN_VALUE -> {
                canonical = requested.getBooleanValue() ? "true" : "false";
                kind = ValueKind.BOOLEAN;
            }
            case TEMPORAL_VALUE -> {
                canonical = temporal(requested.getTemporalValue());
                kind = switch (requested.getTemporalValue().getKind()) {
                    case COMMUNITY_DML_TEMPORAL_KIND_DATE -> ValueKind.DATE;
                    case COMMUNITY_DML_TEMPORAL_KIND_TIME -> ValueKind.TIME;
                    case COMMUNITY_DML_TEMPORAL_KIND_LOCAL_DATETIME ->
                            ValueKind.LOCAL_DATETIME;
                    case COMMUNITY_DML_TEMPORAL_KIND_OFFSET_DATETIME ->
                            ValueKind.OFFSET_DATETIME;
                    case COMMUNITY_DML_TEMPORAL_KIND_UNSPECIFIED, UNRECOGNIZED ->
                            throw RuntimeFailure.validation(
                                    "community.dml_temporal_kind_invalid",
                                    "the Community DML temporal kind is invalid");
                };
            }
            case BINARY_VALUE -> {
                canonical = binary(requested.getBinaryValue().toByteArray());
                kind = ValueKind.BINARY;
            }
            case VALUE_NOT_SET -> throw RuntimeFailure.validation(
                    "community.dml_value_required", "a Community DML value is required");
            default -> throw RuntimeFailure.validation(
                    "community.dml_value_invalid", "the Community DML value kind is invalid");
        }
        String processorValue = kind == ValueKind.BOOLEAN
                        && requiresNumericBooleanInput(databaseType)
                ? (canonical.equals("true") ? "1" : "0")
                : canonical;
        String rendered = dialect.renderValue(column, processorValue);
        validateRenderedValue(kind, canonical, rendered, column, databaseType);
        if (kind == ValueKind.BOOLEAN
                && normalizeDatabaseType(databaseType).equals("H2")) {
            return canonical.equals("true") ? "TRUE" : "FALSE";
        }
        return rendered;
    }

    private static String decimal(String requested) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(requested, MAX_DECIMAL_BYTES, "dml_decimal_value");
        if (!DECIMAL.matcher(requested).matches()) {
            throw RuntimeFailure.validation(
                    "community.dml_decimal_invalid", "the Community DML decimal is invalid");
        }
        try {
            BigDecimal parsed = new BigDecimal(requested).stripTrailingZeros();
            return parsed.signum() == 0 ? "0" : parsed.toPlainString();
        } catch (NumberFormatException failure) {
            throw RuntimeFailure.validation(
                    "community.dml_decimal_invalid", "the Community DML decimal is invalid");
        }
    }

    private static String temporal(CommunityDmlTemporal requested) throws RuntimeFailure {
        String iso8601 = requested.getIso8601();
        ProtocolLimits.requireNonBlankUtf8(
                iso8601, MAX_TEMPORAL_BYTES, "dml_temporal_value");
        try {
            return switch (requested.getKind()) {
                case COMMUNITY_DML_TEMPORAL_KIND_DATE -> LocalDate.parse(iso8601).toString();
                case COMMUNITY_DML_TEMPORAL_KIND_TIME -> LocalTime.parse(iso8601).toString();
                case COMMUNITY_DML_TEMPORAL_KIND_LOCAL_DATETIME ->
                        LocalDateTime.parse(iso8601).toString().replace('T', ' ');
                case COMMUNITY_DML_TEMPORAL_KIND_OFFSET_DATETIME -> {
                    OffsetDateTime parsed = OffsetDateTime.parse(iso8601);
                    yield parsed.toLocalDateTime().toString().replace('T', ' ')
                            + " "
                            + parsed.getOffset();
                }
                case COMMUNITY_DML_TEMPORAL_KIND_UNSPECIFIED, UNRECOGNIZED ->
                        throw RuntimeFailure.validation(
                                "community.dml_temporal_kind_invalid",
                                "the Community DML temporal kind is invalid");
            };
        } catch (DateTimeException failure) {
            throw RuntimeFailure.validation(
                    "community.dml_temporal_invalid",
                    "the Community DML temporal value is not valid ISO-8601");
        }
    }

    private static String binary(byte[] bytes) throws RuntimeFailure {
        if (bytes.length > MAX_VALUE_BYTES) {
            throw RuntimeFailure.limit("dml_binary_value", MAX_VALUE_BYTES);
        }
        char[] encoded = new char[2 + bytes.length * 2];
        encoded[0] = '0';
        encoded[1] = 'x';
        for (int index = 0; index < bytes.length; index++) {
            int value = bytes[index] & 0xff;
            encoded[2 + index * 2] = HEX[value >>> 4];
            encoded[3 + index * 2] = HEX[value & 0x0f];
        }
        return new String(encoded);
    }

    private static void validateRenderedValue(
            ValueKind kind,
            String canonical,
            String rendered,
            Column column,
            String databaseType)
            throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                rendered, ProtocolLimits.MAX_SQL_BYTES, "rendered_dml_value");
        if (kind == ValueKind.STRING) {
            if (!isSafeStringLiteral(canonical, rendered)) {
                throw valueNotSupported();
            }
            return;
        }
        if (kind == ValueKind.NULL) {
            if (!rendered.equalsIgnoreCase("NULL")) {
                throw valueNotSupported();
            }
            return;
        }
        if (kind == ValueKind.DECIMAL) {
            if (!isCompatibleDecimalType(column.dataTypeName())
                    || (!rendered.equals(canonical)
                            && !isSafeStringLiteral(canonical, rendered))) {
                throw valueNotSupported();
            }
            return;
        }
        if (kind == ValueKind.BOOLEAN) {
            if (!isCompatibleBooleanType(column.dataTypeName())
                    || !isSupportedBooleanLiteral(databaseType, canonical, rendered)) {
                throw valueNotSupported();
            }
            return;
        }
        if (kind == ValueKind.DATE
                || kind == ValueKind.TIME
                || kind == ValueKind.LOCAL_DATETIME
                || kind == ValueKind.OFFSET_DATETIME) {
            if (!isSupportedTemporalLiteral(kind, column, canonical, rendered)) {
                throw valueNotSupported();
            }
            return;
        }
        if (kind == ValueKind.BINARY) {
            if (!isCompatibleBinaryType(column.dataTypeName())
                    || !isSupportedBinaryLiteral(canonical, rendered)) {
                throw valueNotSupported();
            }
            return;
        }
        throw valueNotSupported();
    }

    private static boolean isSafeStringLiteral(String canonical, String rendered) {
        if (canonical.indexOf('\\') >= 0 || canonical.indexOf('\0') >= 0) {
            return false;
        }
        int quote = 0;
        if (rendered.length() >= 2
                && (rendered.charAt(0) == 'N'
                        || rendered.charAt(0) == 'n'
                        || rendered.charAt(0) == 'E'
                        || rendered.charAt(0) == 'e')) {
            quote = 1;
        }
        if (quote >= rendered.length() || rendered.charAt(quote) != '\'') {
            return false;
        }
        StringBuilder decoded = new StringBuilder(canonical.length());
        for (int index = quote + 1; index < rendered.length(); index++) {
            char character = rendered.charAt(index);
            if (character == '\\') {
                return false;
            }
            if (character != '\'') {
                decoded.append(character);
                continue;
            }
            if (index + 1 < rendered.length() && rendered.charAt(index + 1) == '\'') {
                decoded.append('\'');
                index++;
                continue;
            }
            return index == rendered.length() - 1 && decoded.toString().equals(canonical);
        }
        return false;
    }

    private static boolean isSupportedBinaryLiteral(String canonical, String rendered) {
        String hex = canonical.substring(2);
        if (rendered.equals(canonical)) {
            return true;
        }
        if (!hex.isEmpty() && rendered.equals("'" + hex + "'")) {
            return true;
        }
        return rendered.equals("E'\\\\x" + hex + "'::bytea");
    }

    private static boolean isSupportedBooleanLiteral(
            String databaseType, String canonical, String rendered) {
        String expectedBit = canonical.equals("true") ? "1" : "0";
        if (rendered.equals(expectedBit)) {
            return true;
        }
        String normalizedDatabaseType = normalizeDatabaseType(databaseType);
        if (isSafeStringLiteral(expectedBit, rendered)
                && Set.of("MYSQL", "MARIADB", "SQLSERVER")
                        .contains(normalizedDatabaseType)) {
            return true;
        }
        if (rendered.equalsIgnoreCase(canonical)) {
            return Set.of(
                            "H2",
                            "POSTGRESQL",
                            "POSTGRES",
                            "MYSQL",
                            "MARIADB",
                            "SQLITE",
                            "DUCKDB",
                            "OPENGAUSS",
                            "GAUSSDB",
                            "KINGBASE")
                    .contains(normalizedDatabaseType);
        }
        if (rendered.equals("b'" + expectedBit + "'")) {
            return normalizedDatabaseType.equals("MYSQL")
                    || normalizedDatabaseType.equals("MARIADB");
        }
        if (isSafeStringLiteral(canonical, rendered)) {
            return Set.of("H2", "POSTGRESQL", "POSTGRES", "OPENGAUSS", "GAUSSDB", "KINGBASE")
                    .contains(normalizedDatabaseType);
        }
        return false;
    }

    private static boolean requiresNumericBooleanInput(String databaseType) {
        return Set.of("MYSQL", "MARIADB", "SQLSERVER")
                .contains(normalizeDatabaseType(databaseType));
    }

    private static boolean isSupportedTemporalLiteral(
            ValueKind kind, Column column, String canonical, String rendered) {
        if (!isCompatibleTemporalType(kind, column.dataTypeName())
                || !isCompatibleTemporalScale(kind, column.scale(), canonical)) {
            return false;
        }
        if (isSafeStringLiteral(canonical, rendered)) {
            return true;
        }
        int scale = column.scale();
        String oracleExpression = switch (kind) {
            case DATE, LOCAL_DATETIME -> scale == 0
                    ? "TO_DATE('" + canonical + "', 'SYYYY-MM-DD HH24:MI:SS')"
                    : "TO_TIMESTAMP('"
                            + canonical
                            + "', 'SYYYY-MM-DD HH24:MI:SS.FF"
                            + scale
                            + "')";
            case OFFSET_DATETIME -> scale == 0
                    ? "TO_TIMESTAMP_TZ('"
                            + canonical
                            + "', 'SYYYY-MM-DD HH24:MI:SS TZR')"
                    : "TO_TIMESTAMP_TZ('"
                            + canonical
                            + "', 'SYYYY-MM-DD HH24:MI:SS.FF"
                            + scale
                            + " TZR')";
            case TIME -> "";
            default -> "";
        };
        return !oracleExpression.isEmpty() && rendered.equalsIgnoreCase(oracleExpression);
    }

    private static boolean isCompatibleTemporalType(ValueKind kind, String dataTypeName) {
        String type = dataTypeName.trim().toUpperCase(Locale.ROOT);
        boolean offset = type.contains("WITH TIME ZONE")
                || type.contains("TIMESTAMPTZ")
                || type.contains("DATETIMEOFFSET")
                || type.contains("TIMESTAMP_TZ");
        boolean localTimeZone = type.contains("WITH LOCAL TIME ZONE");
        boolean dateTime = type.contains("TIMESTAMP")
                || type.contains("DATETIME")
                || type.contains("SMALLDATETIME");
        boolean date = type.equals("DATE") || type.startsWith("DATE(") || type.startsWith("DATE ");
        boolean time = (type.equals("TIME") || type.startsWith("TIME(") || type.startsWith("TIME "))
                && !type.contains("TIMESTAMP");
        return switch (kind) {
            case DATE -> date;
            case TIME -> time && !offset;
            case LOCAL_DATETIME -> (dateTime && (!offset || localTimeZone)) || date;
            case OFFSET_DATETIME -> offset && !localTimeZone;
            default -> false;
        };
    }

    private static boolean isCompatibleTemporalScale(
            ValueKind kind, int scale, String canonical) {
        if (scale < 0 || scale > 9) {
            return false;
        }
        if (kind == ValueKind.DATE) {
            return true;
        }
        int decimalPoint = canonical.indexOf('.');
        if (decimalPoint < 0) {
            return true;
        }
        int end = decimalPoint + 1;
        while (end < canonical.length() && Character.isDigit(canonical.charAt(end))) {
            end++;
        }
        return end - decimalPoint - 1 <= scale;
    }

    private static boolean isCompatibleBooleanType(String dataTypeName) {
        String type = dataTypeName.trim().toUpperCase(Locale.ROOT);
        return !type.contains("VARYING")
                && Set.of("BOOL", "BOOLEAN", "BIT").contains(baseDataType(type));
    }

    private static boolean isCompatibleDecimalType(String dataTypeName) {
        String type = dataTypeName.trim().toUpperCase(Locale.ROOT);
        return Set.of(
                        "TINYINT",
                        "SMALLINT",
                        "MEDIUMINT",
                        "BIGINT",
                        "INTEGER",
                        "INT",
                        "INT2",
                        "INT4",
                        "INT8",
                        "DECIMAL",
                        "DEC",
                        "NUMERIC",
                        "NUMBER",
                        "FLOAT",
                        "FLOAT4",
                        "FLOAT8",
                        "DOUBLE",
                        "REAL",
                        "MONEY",
                        "SMALLMONEY",
                        "SERIAL",
                        "BIGSERIAL",
                        "BINARY_FLOAT",
                        "BINARY_DOUBLE")
                .contains(baseDataType(type));
    }

    private static boolean isCompatibleBinaryType(String dataTypeName) {
        String type = dataTypeName.trim().toUpperCase(Locale.ROOT);
        if (type.startsWith("BIT VARYING") || type.startsWith("LONG RAW")) {
            return true;
        }
        return Set.of(
                        "BINARY",
                        "VARBINARY",
                        "LONGVARBINARY",
                        "BLOB",
                        "TINYBLOB",
                        "MEDIUMBLOB",
                        "LONGBLOB",
                        "BYTEA",
                        "RAW",
                        "IMAGE")
                .contains(baseDataType(type));
    }

    private static String baseDataType(String type) {
        int end = type.length();
        int parenthesis = type.indexOf('(');
        if (parenthesis >= 0) {
            end = Math.min(end, parenthesis);
        }
        int space = type.indexOf(' ');
        if (space >= 0) {
            end = Math.min(end, space);
        }
        return type.substring(0, end).trim();
    }

    private static String normalizeDatabaseType(String databaseType) {
        return databaseType
                .trim()
                .toUpperCase(Locale.ROOT)
                .replace("-", "")
                .replace("_", "")
                .replace(" ", "");
    }

    private static Target target(Dialect dialect, CommunityDmlTarget requested)
            throws ReflectiveOperationException, RuntimeFailure {
        String database = identifier(
                dialect,
                requested.hasDatabaseName() ? requested.getDatabaseName() : "",
                true,
                "database_name");
        String schema = identifier(
                dialect,
                requested.hasSchemaName() ? requested.getSchemaName() : "",
                true,
                "schema_name");
        String table = identifier(dialect, requested.getTableName(), false, "table_name");
        return new Target(database, schema, table);
    }

    private static String identifier(
            Dialect dialect, String requested, boolean optional, String field)
            throws ReflectiveOperationException, RuntimeFailure {
        if (optional && requested.isEmpty()) {
            return "";
        }
        ProtocolLimits.requireNonBlankUtf8(requested, MAX_IDENTIFIER_BYTES, field);
        requireRawIdentifier(requested);
        String quoted = dialect.quoteIdentifier(requested);
        ProtocolLimits.requireNonBlankUtf8(quoted, MAX_IDENTIFIER_BYTES, "quoted_" + field);
        requireQuotedIdentifier(requested, quoted);
        return quoted;
    }

    private static void requireRawIdentifier(String value) throws RuntimeFailure {
        requireNoAsciiControl(value, "identifier");
        if (value.indexOf('.') >= 0
                || value.indexOf('\'') >= 0
                || value.indexOf('"') >= 0
                || value.indexOf('`') >= 0
                || value.indexOf('[') >= 0
                || value.indexOf(']') >= 0
                || value.indexOf(';') >= 0
                || value.contains("--")
                || value.contains("/*")
                || value.contains("*/")) {
            throw RuntimeFailure.validation(
                    "community.dml_identifier_invalid",
                    "Community DML identifiers must be unquoted single segments");
        }
    }

    private static void requireQuotedIdentifier(String requested, String value)
            throws RuntimeFailure {
        requireNoAsciiControl(value, "quoted_identifier");
        boolean exact = value.equals(requested)
                || value.equals("\"" + requested + "\"")
                || value.equals("`" + requested + "`")
                || value.equals("[" + requested + "]");
        if (!exact
                || value.indexOf('.') >= 0
                || value.indexOf('\'') >= 0
                || value.indexOf(';') >= 0
                || value.contains("--")
                || value.contains("/*")
                || value.contains("*/")) {
            throw RuntimeFailure.validation(
                    "community.dml_identifier_invalid",
                    "the Community identifier processor returned an invalid identifier");
        }
    }

    private static void requireNoAsciiControl(String value, String field)
            throws RuntimeFailure {
        for (int index = 0; index < value.length(); index++) {
            char character = value.charAt(index);
            if (character <= 0x1f || character == 0x7f) {
                throw RuntimeFailure.validation(
                        "community.dml_" + field + "_invalid",
                        "the Community DML " + field + " contains a control character");
            }
        }
    }

    private static List<String> identifiers(List<Column> columns) {
        return columns.stream().map(Column::identifier).toList();
    }

    static void validateRequest(BuildCommunityDmlRequest request)
            throws RuntimeFailure {
        if (request == null) {
            throw RuntimeFailure.validation(
                    "community.dml_request_required", "the Community DML request is required");
        }
        ProtocolLimits.requireNonBlankUtf8(
                request.getDatabaseType(),
                CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES.getNumber(),
                "database_type");
        if (!request.hasTarget()) {
            throw RuntimeFailure.validation(
                    "community.dml_target_required", "a Community DML target is required");
        }
        if (request.getStatementCase() == BuildCommunityDmlRequest.StatementCase.STATEMENT_NOT_SET) {
            throw RuntimeFailure.validation(
                    "community.dml_statement_required", "a Community DML statement is required");
        }
    }

    private static void requireCount(int count, int maximum, String field)
            throws RuntimeFailure {
        if (count > maximum) {
            throw RuntimeFailure.limit(field, maximum);
        }
    }

    private static void requireValueCount(long count) throws RuntimeFailure {
        if (count > MAX_VALUES) {
            throw RuntimeFailure.limit("Community DML values", MAX_VALUES);
        }
    }

    private static RuntimeFailure notSupported() {
        return RuntimeFailure.validation(
                "community.dml_builder_not_supported",
                "the selected Community plugin does not support bounded DML generation");
    }

    private static RuntimeFailure valueNotSupported() {
        return RuntimeFailure.validation(
                "community.dml_value_not_supported",
                "the selected Community value processor cannot preserve this typed value");
    }

    private static RuntimeFailure failed(Throwable cause) {
        return RuntimeFailure.internal(
                "community.dml_builder_failed",
                "the Community DML builder failed internally",
                cause);
    }

    private static Throwable invocationCause(InvocationTargetException failure) {
        return failure.getCause() == null ? failure : failure.getCause();
    }

    interface Dialect {
        String quoteIdentifier(String identifier) throws ReflectiveOperationException;

        String renderValue(Column column, String value) throws ReflectiveOperationException;

        String buildSingleInsert(Target target, List<String> columns, List<String> values)
                throws ReflectiveOperationException;

        String buildMultiInsert(
                Target target, List<String> columns, List<List<String>> rows)
                throws ReflectiveOperationException;

        String buildUpdate(
                Target target, Map<String, String> assignments, Map<String, String> predicates)
                throws ReflectiveOperationException;
    }

    record Column(String identifier, String dataTypeName, int precision, int scale) {}

    record Target(String databaseName, String schemaName, String tableName) {}

    private enum ValueKind {
        NULL,
        STRING,
        DECIMAL,
        BOOLEAN,
        DATE,
        TIME,
        LOCAL_DATETIME,
        OFFSET_DATETIME,
        BINARY
    }

    private static final class ReflectiveDialect implements Dialect {
        private final Object dmlBuilder;
        private final Object valueProcessor;
        private final Object identifierProcessor;
        private final Class<?> dataTypeClass;
        private final Class<?> sqlDataValueClass;
        private final Class<?> singleInsertRequestClass;
        private final Class<?> multiInsertRequestClass;
        private final Class<?> updateRequestClass;
        private final boolean sqlServer;

        private ReflectiveDialect(ClassLoader loader, Object plugin)
                throws ReflectiveOperationException {
            if (plugin == null) {
                throw new UnsupportedOperationException("Community plugin is unavailable");
            }
            Object sqlBuilder = invoke(plugin, "getSqlBuilder");
            valueProcessor = invoke(plugin, "getValueProcessor");
            identifierProcessor = invoke(plugin, "getSQLIdentifierProcessor");
            if (sqlBuilder == null || valueProcessor == null || identifierProcessor == null) {
                throw new UnsupportedOperationException("Community DML components are unavailable");
            }
            dmlBuilder = invoke(sqlBuilder, "dml");
            if (dmlBuilder == null) {
                throw new UnsupportedOperationException("Community DML builder is unavailable");
            }
            dataTypeClass = Class.forName(DATA_TYPE_CLASS, true, loader);
            sqlDataValueClass = Class.forName(SQL_DATA_VALUE_CLASS, true, loader);
            singleInsertRequestClass = Class.forName(SINGLE_INSERT_REQUEST_CLASS, true, loader);
            multiInsertRequestClass = Class.forName(MULTI_INSERT_REQUEST_CLASS, true, loader);
            updateRequestClass = Class.forName(UPDATE_REQUEST_CLASS, true, loader);
            sqlServer = plugin.getClass().getName().startsWith(SQL_SERVER_PLUGIN_PREFIX);
        }

        @Override
        public String quoteIdentifier(String identifier) throws ReflectiveOperationException {
            return stringResult(invoke(
                    identifierProcessor,
                    "quoteIdentifier",
                    new Class<?>[] {String.class},
                    identifier));
        }

        @Override
        public String renderValue(Column column, String value)
                throws ReflectiveOperationException {
            Object dataType = dataTypeClass.getDeclaredConstructor().newInstance();
            invokeSetter(dataType, "setDataTypeName", String.class, column.dataTypeName());
            invokeSetter(dataType, "setPrecision", Integer.class, column.precision());
            invokeSetter(dataType, "setScale", Integer.class, column.scale());
            Object sqlDataValue = sqlDataValueClass.getDeclaredConstructor().newInstance();
            invokeSetter(sqlDataValue, "setValue", String.class, value);
            invokeSetter(sqlDataValue, "setDataType", dataTypeClass, dataType);
            return stringResult(invoke(
                    valueProcessor,
                    "getSqlValueString",
                    new Class<?>[] {sqlDataValueClass},
                    sqlDataValue));
        }

        @Override
        public String buildSingleInsert(
                Target target, List<String> columns, List<String> values)
                throws ReflectiveOperationException {
            Object request = request(singleInsertRequestClass, target);
            invokeSetter(request, "setColumnList", List.class, insertColumns(columns));
            invokeSetter(request, "setValueList", List.class, values);
            return stringResult(invoke(
                    dmlBuilder,
                    "buildInsert",
                    new Class<?>[] {singleInsertRequestClass},
                    request));
        }

        @Override
        public String buildMultiInsert(
                Target target, List<String> columns, List<List<String>> rows)
                throws ReflectiveOperationException {
            Object request = request(multiInsertRequestClass, target);
            invokeSetter(request, "setColumnList", List.class, insertColumns(columns));
            invokeSetter(request, "setValueLists", List.class, rows);
            return stringResult(invoke(
                    dmlBuilder,
                    "buildBatchInsert",
                    new Class<?>[] {multiInsertRequestClass},
                    request));
        }

        @Override
        public String buildUpdate(
                Target target, Map<String, String> assignments, Map<String, String> predicates)
                throws ReflectiveOperationException {
            Object request = request(updateRequestClass, target);
            invokeSetter(request, "setRow", Map.class, assignments);
            invokeSetter(request, "setPrimaryKeyMap", Map.class, predicates);
            return stringResult(invoke(
                    dmlBuilder,
                    "buildUpdate",
                    new Class<?>[] {updateRequestClass},
                    request));
        }

        private Object request(Class<?> type, Target target)
                throws ReflectiveOperationException {
            Object request = type.getDeclaredConstructor().newInstance();
            invokeSetter(
                    request,
                    "setDatabaseName",
                    String.class,
                    builderTargetIdentifier(target.databaseName()));
            invokeSetter(
                    request,
                    "setSchemaName",
                    String.class,
                    builderTargetIdentifier(target.schemaName()));
            invokeSetter(
                    request,
                    "setTableName",
                    String.class,
                    builderTargetIdentifier(target.tableName()));
            return request;
        }

        private List<String> insertColumns(List<String> columns) {
            if (!sqlServer) {
                return columns;
            }
            return columns.stream().map(ReflectiveDialect::unquoteSquareIdentifier).toList();
        }

        private String builderTargetIdentifier(String identifier) {
            return sqlServer ? unquoteSquareIdentifier(identifier) : identifier;
        }

        private static String unquoteSquareIdentifier(String identifier) {
            if (identifier.length() >= 2
                    && identifier.charAt(0) == '['
                    && identifier.charAt(identifier.length() - 1) == ']') {
                return identifier.substring(1, identifier.length() - 1);
            }
            return identifier;
        }
    }

    private static Object invoke(Object target, String method)
            throws ReflectiveOperationException {
        return invoke(target, method, new Class<?>[0]);
    }

    private static Object invoke(
            Object target, String method, Class<?>[] parameterTypes, Object... arguments)
            throws ReflectiveOperationException {
        Method reflected = target.getClass().getMethod(method, parameterTypes);
        return reflected.invoke(target, arguments);
    }

    private static void invokeSetter(
            Object target, String method, Class<?> parameterType, Object value)
            throws ReflectiveOperationException {
        invoke(target, method, new Class<?>[] {parameterType}, value);
    }

    private static String stringResult(Object value) {
        if (!(value instanceof String string)) {
            throw new IllegalStateException("Community DML component returned a non-string value");
        }
        return string;
    }
}

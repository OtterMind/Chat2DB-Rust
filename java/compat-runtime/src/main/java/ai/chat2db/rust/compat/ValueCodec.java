package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.ColumnNullability;
import ai.chat2db.rust.compat.protocol.v1.JdbcColumn;
import ai.chat2db.rust.compat.protocol.v1.JdbcNull;
import ai.chat2db.rust.compat.protocol.v1.JdbcParameter;
import ai.chat2db.rust.compat.protocol.v1.JdbcValue;
import ai.chat2db.rust.compat.protocol.v1.JdbcValueType;
import ai.chat2db.rust.compat.protocol.v1.OpaqueValue;
import com.google.protobuf.ByteString;
import com.google.protobuf.CodedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.SQLXML;
import java.sql.Types;
import java.time.DateTimeException;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.time.OffsetDateTime;
import java.time.OffsetTime;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.UUID;

/** Loss-aware conversion between JDBC values and the typed wire representation. */
final class ValueCodec {

    private static final BigInteger MAX_UNSIGNED_LONG =
            BigInteger.ONE.shiftLeft(Long.SIZE).subtract(BigInteger.ONE);

    private ValueCodec() {
    }

    static List<JdbcColumn> columns(ResultSetMetaData metadata, int maximumEncodedBytes)
            throws SQLException, RuntimeFailure {
        int count = metadata.getColumnCount();
        if (count > ProtocolLimits.MAX_COLUMNS) {
            throw RuntimeFailure.limit("columns", ProtocolLimits.MAX_COLUMNS);
        }
        if (maximumEncodedBytes <= 0) {
            throw RuntimeFailure.limit("query_metadata", Math.max(0, maximumEncodedBytes));
        }
        java.util.ArrayList<JdbcColumn> columns = new java.util.ArrayList<>(count);
        int acceptedEncodedBytes = 0;
        for (int index = 1; index <= count; index++) {
            int jdbcType = metadata.getColumnType(index);
            boolean signed = metadata.isSigned(index);
            JdbcColumn.Builder column = JdbcColumn.newBuilder()
                    .setOrdinal(index)
                    .setJdbcType(jdbcType)
                    .setNullability(nullability(metadata.isNullable(index)))
                    .setSigned(signed);
            int precision = metadata.getPrecision(index);
            if (precision >= 0) {
                column.setPrecision(precision);
            }
            int scale = metadata.getScale(index);
            if (scale >= 0) {
                column.setScale(scale);
            }
            int displaySize = metadata.getColumnDisplaySize(index);
            if (displaySize >= 0) {
                column.setDisplaySize(displaySize);
            }
            ensureMetadataFits(
                    acceptedEncodedBytes, column, maximumEncodedBytes);

            String typeName = metadataScalar(
                    metadata.getColumnTypeName(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            column.setJdbcTypeName(typeName)
                    .setValueType(valueType(jdbcType, typeName, signed));
            ensureMetadataFits(
                    acceptedEncodedBytes, column, maximumEncodedBytes);
            setMetadataString(
                    column::setLabel,
                    metadata.getColumnLabel(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            setMetadataString(
                    column::setName,
                    metadata.getColumnName(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            setOptionalMetadataString(
                    column::setCatalogName,
                    metadata.getCatalogName(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            setOptionalMetadataString(
                    column::setSchemaName,
                    metadata.getSchemaName(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            setOptionalMetadataString(
                    column::setTableName,
                    metadata.getTableName(index),
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
            JdbcColumn built = column.build();
            acceptedEncodedBytes += encodedColumnSize(built);
            columns.add(built);
        }
        return List.copyOf(columns);
    }

    static JdbcValue read(ResultSet resultSet, JdbcColumn column, int maximumContentBytes)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        int index = column.getOrdinal();
        String typeName = column.getJdbcTypeName();
        int jdbcType = column.getJdbcType();
        if (isJson(typeName)) {
            return readerValue(
                    resultSet.getCharacterStream(index),
                    JdbcValue.newBuilder()::setJsonValue,
                    maximumContentBytes,
                    "json_value");
        }
        if (isUuid(typeName)) {
            return readerValue(
                    resultSet.getCharacterStream(index),
                    JdbcValue.newBuilder()::setUuidValue,
                    maximumContentBytes,
                    "uuid_value");
        }

        return switch (jdbcType) {
            case Types.BOOLEAN, Types.BIT -> booleanValue(resultSet, index);
            case Types.TINYINT, Types.SMALLINT, Types.INTEGER, Types.BIGINT -> integerValue(
                    resultSet.getBigDecimal(index), column.hasSigned() && !column.getSigned());
            case Types.REAL -> float32Value(resultSet, index);
            case Types.FLOAT, Types.DOUBLE -> float64Value(resultSet, index);
            case Types.NUMERIC, Types.DECIMAL -> decimalValue(
                    resultSet.getBigDecimal(index), maximumContentBytes);
            case Types.BINARY, Types.VARBINARY, Types.LONGVARBINARY -> binaryValue(
                    resultSet.getBinaryStream(index), maximumContentBytes, "binary_value");
            case Types.BLOB -> blobValue(resultSet.getBlob(index), maximumContentBytes);
            case Types.CLOB, Types.NCLOB -> clobValue(resultSet.getClob(index), maximumContentBytes);
            case Types.DATE -> temporalValue(
                    resultSet.getObject(index), JdbcValue.newBuilder()::setDateValue, "date_value", 0);
            case Types.TIME -> temporalValue(
                    resultSet.getObject(index), JdbcValue.newBuilder()::setTimeValue, "time_value", 1);
            case Types.TIME_WITH_TIMEZONE -> temporalValue(
                    resultSet.getObject(index), JdbcValue.newBuilder()::setTimeValue, "time_value", 2);
            case Types.TIMESTAMP -> temporalValue(
                    resultSet.getObject(index),
                    JdbcValue.newBuilder()::setTimestampValue,
                    "timestamp_value",
                    3);
            case Types.TIMESTAMP_WITH_TIMEZONE -> temporalValue(
                    resultSet.getObject(index),
                    JdbcValue.newBuilder()::setTimestampWithTimeZoneValue,
                    "timestamp_with_time_zone_value",
                    4);
            case Types.CHAR,
                    Types.VARCHAR,
                    Types.LONGVARCHAR,
                    Types.NCHAR,
                    Types.NVARCHAR,
                    Types.LONGNVARCHAR -> readerValue(
                    resultSet.getCharacterStream(index),
                    JdbcValue.newBuilder()::setTextValue,
                    maximumContentBytes,
                    "text_value");
            case Types.SQLXML -> sqlXmlValue(resultSet.getSQLXML(index), typeName, maximumContentBytes);
            default -> opaqueValue(
                    typeName, resultSet.getCharacterStream(index), maximumContentBytes);
        };
    }

    static void bind(PreparedStatement statement, List<JdbcParameter> parameters)
            throws SQLException, RuntimeFailure {
        if (parameters.size() > ProtocolLimits.MAX_PARAMETERS) {
            throw RuntimeFailure.limit("parameters", ProtocolLimits.MAX_PARAMETERS);
        }
        Set<Integer> positions = new HashSet<>();
        for (JdbcParameter parameter : parameters) {
            int position = parameter.getPosition();
            if (position <= 0 || position > ProtocolLimits.MAX_PARAMETERS) {
                throw RuntimeFailure.validation(
                        "database.invalid_parameter_position",
                        "parameter position must be a one-based JDBC ordinal");
            }
            if (!positions.add(position)) {
                throw RuntimeFailure.validation(
                        "database.duplicate_parameter_position",
                        "parameter positions must be unique");
            }
            if (!parameter.hasValue()) {
                throw RuntimeFailure.validation(
                        "database.parameter_value_required", "every parameter requires a value");
            }
            bindValue(statement, position, parameter);
        }
    }

    private static void bindValue(PreparedStatement statement, int position, JdbcParameter parameter)
            throws SQLException, RuntimeFailure {
        JdbcValue value = parameter.getValue();
        try {
            switch (value.getValueCase()) {
                case NULL_VALUE -> {
                    int jdbcType = parameter.hasJdbcType() ? parameter.getJdbcType() : Types.NULL;
                    if (parameter.hasJdbcTypeName() && !parameter.getJdbcTypeName().isBlank()) {
                        ProtocolLimits.requireUtf8(
                                parameter.getJdbcTypeName(),
                                ProtocolLimits.MAX_SCALAR_BYTES,
                                "jdbc_type_name");
                        statement.setNull(position, jdbcType, parameter.getJdbcTypeName());
                    } else {
                        statement.setNull(position, jdbcType);
                    }
                }
                case BOOLEAN_VALUE -> statement.setBoolean(position, value.getBooleanValue());
                case SIGNED_INTEGER_VALUE ->
                        statement.setLong(position, value.getSignedIntegerValue());
                case UNSIGNED_INTEGER_VALUE -> statement.setBigDecimal(
                        position,
                        new BigDecimal(new BigInteger(
                                Long.toUnsignedString(value.getUnsignedIntegerValue()))));
                case FLOAT32_VALUE -> statement.setFloat(position, value.getFloat32Value());
                case FLOAT64_VALUE -> statement.setDouble(position, value.getFloat64Value());
                case DECIMAL_VALUE -> {
                    ProtocolLimits.requireUtf8(
                            value.getDecimalValue(), ProtocolLimits.MAX_SCALAR_BYTES, "decimal_value");
                    statement.setBigDecimal(position, new BigDecimal(value.getDecimalValue()));
                }
                case TEXT_VALUE -> {
                    ProtocolLimits.requireUtf8(
                            value.getTextValue(), ProtocolLimits.MAX_SCALAR_BYTES, "text_value");
                    statement.setString(position, value.getTextValue());
                }
                case BINARY_VALUE -> {
                    if (value.getBinaryValue().size() > ProtocolLimits.MAX_SCALAR_BYTES) {
                        throw RuntimeFailure.limit("binary_value", ProtocolLimits.MAX_SCALAR_BYTES);
                    }
                    statement.setBytes(position, value.getBinaryValue().toByteArray());
                }
                case DATE_VALUE -> {
                    requireParameterScalar(value.getDateValue(), "date_value");
                    statement.setObject(position, LocalDate.parse(value.getDateValue()));
                }
                case TIME_VALUE -> {
                    requireParameterScalar(value.getTimeValue(), "time_value");
                    statement.setObject(position, parseTime(value.getTimeValue()));
                }
                case TIMESTAMP_VALUE -> {
                    requireParameterScalar(value.getTimestampValue(), "timestamp_value");
                    statement.setObject(position, LocalDateTime.parse(value.getTimestampValue()));
                }
                case TIMESTAMP_WITH_TIME_ZONE_VALUE -> {
                    requireParameterScalar(
                            value.getTimestampWithTimeZoneValue(),
                            "timestamp_with_time_zone_value");
                    statement.setObject(
                            position, OffsetDateTime.parse(value.getTimestampWithTimeZoneValue()));
                }
                case JSON_VALUE -> {
                    ProtocolLimits.requireUtf8(
                            value.getJsonValue(), ProtocolLimits.MAX_SCALAR_BYTES, "json_value");
                    statement.setString(position, value.getJsonValue());
                }
                case UUID_VALUE -> {
                    requireParameterScalar(value.getUuidValue(), "uuid_value");
                    statement.setObject(position, UUID.fromString(value.getUuidValue()));
                }
                case OPAQUE_VALUE -> throw RuntimeFailure.validation(
                        "database.opaque_parameter_unsupported",
                        "opaque values cannot be used as JDBC parameters");
                case VALUE_NOT_SET -> throw RuntimeFailure.validation(
                        "database.parameter_value_required", "every parameter requires a value");
            }
        } catch (IllegalArgumentException | DateTimeException invalid) {
            throw RuntimeFailure.validation(
                    "database.invalid_parameter_value",
                    "the typed parameter value is not in its canonical format");
        }
    }

    private static JdbcValue booleanValue(ResultSet resultSet, int index) throws SQLException {
        boolean value = resultSet.getBoolean(index);
        return resultSet.wasNull()
                ? nullValue()
                : JdbcValue.newBuilder().setBooleanValue(value).build();
    }

    private static JdbcValue float32Value(ResultSet resultSet, int index) throws SQLException {
        float value = resultSet.getFloat(index);
        return resultSet.wasNull()
                ? nullValue()
                : JdbcValue.newBuilder().setFloat32Value(value).build();
    }

    private static JdbcValue float64Value(ResultSet resultSet, int index) throws SQLException {
        double value = resultSet.getDouble(index);
        return resultSet.wasNull()
                ? nullValue()
                : JdbcValue.newBuilder().setFloat64Value(value).build();
    }

    private static JdbcValue integerValue(BigDecimal value, boolean unsigned) throws RuntimeFailure {
        if (value == null) {
            return nullValue();
        }
        BigInteger integer;
        try {
            integer = value.toBigIntegerExact();
        } catch (ArithmeticException invalid) {
            throw RuntimeFailure.validation(
                    "database.invalid_integer_value", "the driver returned a non-integral integer value");
        }
        if (unsigned) {
            if (integer.signum() < 0 || integer.compareTo(MAX_UNSIGNED_LONG) > 0) {
                throw RuntimeFailure.validation(
                        "database.integer_out_of_range", "the unsigned integer exceeds uint64");
            }
            return JdbcValue.newBuilder().setUnsignedIntegerValue(integer.longValue()).build();
        }
        if (integer.bitLength() > 63) {
            throw RuntimeFailure.validation(
                    "database.integer_out_of_range", "the signed integer exceeds sint64");
        }
        return JdbcValue.newBuilder().setSignedIntegerValue(integer.longValue()).build();
    }

    private static JdbcValue decimalValue(BigDecimal value, int maximumContentBytes)
            throws RuntimeFailure, ValueLimitExceeded {
        if (value == null) {
            return nullValue();
        }
        String decimal = value.toString();
        requireContentLimit(decimal, maximumContentBytes, "decimal_value");
        return textValue(JdbcValue.newBuilder()::setDecimalValue, decimal, "decimal_value");
    }

    private static JdbcValue binaryValue(InputStream source, int maximumContentBytes, String field)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (source == null) {
            return nullValue();
        }
        byte[] bytes = readBytes(source, Math.min(maximumContentBytes, ProtocolLimits.MAX_SCALAR_BYTES), field);
        return JdbcValue.newBuilder().setBinaryValue(ByteString.copyFrom(bytes)).build();
    }

    private static JdbcValue blobValue(Blob blob, int maximumContentBytes)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (blob == null) {
            return nullValue();
        }
        try {
            return binaryValue(blob.getBinaryStream(), maximumContentBytes, "binary_value");
        } finally {
            blob.free();
        }
    }

    private static JdbcValue clobValue(Clob clob, int maximumContentBytes)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (clob == null) {
            return nullValue();
        }
        try {
            return readerValue(
                    clob.getCharacterStream(),
                    JdbcValue.newBuilder()::setTextValue,
                    maximumContentBytes,
                    "text_value");
        } finally {
            clob.free();
        }
    }

    private static JdbcValue sqlXmlValue(SQLXML xml, String typeName, int maximumContentBytes)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (xml == null) {
            return nullValue();
        }
        try {
            return readerValue(
                    xml.getCharacterStream(),
                    isJson(typeName)
                            ? JdbcValue.newBuilder()::setJsonValue
                            : JdbcValue.newBuilder()::setTextValue,
                    maximumContentBytes,
                    isJson(typeName) ? "json_value" : "text_value");
        } finally {
            xml.free();
        }
    }

    private static JdbcValue opaqueValue(String typeName, Reader source, int maximumContentBytes)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (source == null) {
            return nullValue();
        }
        String display = readText(
                source, Math.min(maximumContentBytes, ProtocolLimits.MAX_SCALAR_BYTES), "opaque_value");
        return JdbcValue.newBuilder()
                .setOpaqueValue(OpaqueValue.newBuilder()
                        .setTypeName(scalar(typeName.isBlank() ? "java.lang.Object" : typeName))
                        .setDisplayValue(display))
                .build();
    }

    private static JdbcValue temporalValue(
            Object value,
            java.util.function.Function<String, JdbcValue.Builder> setter,
            String field,
            int kind)
            throws RuntimeFailure {
        if (value == null) {
            return nullValue();
        }
        String rendered = switch (kind) {
            case 0 -> dateString(value);
            case 1 -> timeString(value);
            case 2 -> offsetTimeString(value);
            case 3 -> timestampString(value);
            default -> offsetTimestampString(value);
        };
        return textValue(setter, rendered, field);
    }

    private static JdbcValue readerValue(
            Reader source,
            java.util.function.Function<String, JdbcValue.Builder> setter,
            int maximumContentBytes,
            String field)
            throws SQLException, RuntimeFailure, ValueLimitExceeded {
        if (source == null) {
            return nullValue();
        }
        String value = readText(
                source, Math.min(maximumContentBytes, ProtocolLimits.MAX_SCALAR_BYTES), field);
        return textValue(setter, value, field);
    }

    private static byte[] readBytes(InputStream source, int maximumBytes, String field)
            throws SQLException, ValueLimitExceeded {
        try (InputStream input = source;
                ByteArrayOutputStream output = new ByteArrayOutputStream(Math.min(maximumBytes, 8192))) {
            byte[] buffer = new byte[8192];
            int total = 0;
            while (true) {
                int requested = Math.min(buffer.length, Math.max(1, maximumBytes - total + 1));
                int count = input.read(buffer, 0, requested);
                if (count == -1) {
                    return output.toByteArray();
                }
                if (total > maximumBytes - count) {
                    throw new ValueLimitExceeded(field, maximumBytes);
                }
                output.write(buffer, 0, count);
                total += count;
            }
        } catch (IOException failure) {
            throw new SQLException("the JDBC binary value could not be read", failure);
        }
    }

    private static String readText(Reader source, int maximumBytes, String field)
            throws SQLException, ValueLimitExceeded {
        try (Reader reader = source) {
            StringBuilder output = new StringBuilder(Math.min(maximumBytes, 8192));
            int encodedBytes = 0;
            int pending = -1;
            while (true) {
                int current = pending >= 0 ? pending : reader.read();
                pending = -1;
                if (current == -1) {
                    return output.toString();
                }
                char first = (char) current;
                int additionalBytes;
                if (Character.isHighSurrogate(first)) {
                    int second = reader.read();
                    if (second >= 0 && Character.isLowSurrogate((char) second)) {
                        additionalBytes = 4;
                        if (encodedBytes > maximumBytes - additionalBytes) {
                            throw new ValueLimitExceeded(field, maximumBytes);
                        }
                        output.append(first).append((char) second);
                        encodedBytes += additionalBytes;
                        continue;
                    }
                    pending = second;
                    additionalBytes = 1;
                } else if (first <= 0x7f) {
                    additionalBytes = 1;
                } else if (first <= 0x7ff) {
                    additionalBytes = 2;
                } else if (Character.isLowSurrogate(first)) {
                    additionalBytes = 1;
                } else {
                    additionalBytes = 3;
                }
                if (encodedBytes > maximumBytes - additionalBytes) {
                    throw new ValueLimitExceeded(field, maximumBytes);
                }
                output.append(first);
                encodedBytes += additionalBytes;
            }
        } catch (IOException failure) {
            throw new SQLException("the JDBC text value could not be read", failure);
        }
    }

    private static void requireContentLimit(String value, int maximumBytes, String field)
            throws ValueLimitExceeded {
        if (ProtocolLimits.utf8LengthExceeds(value, maximumBytes)) {
            throw new ValueLimitExceeded(field, maximumBytes);
        }
    }

    private static void requireParameterScalar(String value, String field) throws RuntimeFailure {
        ProtocolLimits.requireUtf8(value, ProtocolLimits.MAX_SCALAR_BYTES, field);
    }

    private static JdbcValue nullValue() {
        return JdbcValue.newBuilder().setNullValue(JdbcNull.getDefaultInstance()).build();
    }

    private static JdbcValue textValue(
            java.util.function.Function<String, JdbcValue.Builder> setter,
            String value,
            String field)
            throws RuntimeFailure {
        String text = value == null ? "" : value;
        ProtocolLimits.requireUtf8(text, ProtocolLimits.MAX_SCALAR_BYTES, field);
        return setter.apply(text).build();
    }

    private static JdbcValueType valueType(int jdbcType, String typeName, boolean signed) {
        if (isJson(typeName)) {
            return JdbcValueType.JDBC_VALUE_TYPE_JSON;
        }
        if (isUuid(typeName)) {
            return JdbcValueType.JDBC_VALUE_TYPE_UUID;
        }
        return switch (jdbcType) {
            case Types.BOOLEAN, Types.BIT -> JdbcValueType.JDBC_VALUE_TYPE_BOOLEAN;
            case Types.TINYINT, Types.SMALLINT, Types.INTEGER, Types.BIGINT -> signed
                    ? JdbcValueType.JDBC_VALUE_TYPE_SIGNED_INTEGER
                    : JdbcValueType.JDBC_VALUE_TYPE_UNSIGNED_INTEGER;
            case Types.REAL -> JdbcValueType.JDBC_VALUE_TYPE_FLOAT32;
            case Types.FLOAT, Types.DOUBLE -> JdbcValueType.JDBC_VALUE_TYPE_FLOAT64;
            case Types.NUMERIC, Types.DECIMAL -> JdbcValueType.JDBC_VALUE_TYPE_DECIMAL;
            case Types.CHAR,
                    Types.VARCHAR,
                    Types.LONGVARCHAR,
                    Types.NCHAR,
                    Types.NVARCHAR,
                    Types.LONGNVARCHAR,
                    Types.CLOB,
                    Types.NCLOB,
                    Types.SQLXML -> JdbcValueType.JDBC_VALUE_TYPE_TEXT;
            case Types.BINARY, Types.VARBINARY, Types.LONGVARBINARY, Types.BLOB ->
                    JdbcValueType.JDBC_VALUE_TYPE_BINARY;
            case Types.DATE -> JdbcValueType.JDBC_VALUE_TYPE_DATE;
            case Types.TIME, Types.TIME_WITH_TIMEZONE -> JdbcValueType.JDBC_VALUE_TYPE_TIME;
            case Types.TIMESTAMP -> JdbcValueType.JDBC_VALUE_TYPE_TIMESTAMP;
            case Types.TIMESTAMP_WITH_TIMEZONE ->
                    JdbcValueType.JDBC_VALUE_TYPE_TIMESTAMP_WITH_TIME_ZONE;
            default -> JdbcValueType.JDBC_VALUE_TYPE_OPAQUE;
        };
    }

    private static ColumnNullability nullability(int jdbcNullability) {
        return switch (jdbcNullability) {
            case ResultSetMetaData.columnNoNulls -> ColumnNullability.COLUMN_NULLABILITY_NO_NULLS;
            case ResultSetMetaData.columnNullable -> ColumnNullability.COLUMN_NULLABILITY_NULLABLE;
            default -> ColumnNullability.COLUMN_NULLABILITY_UNKNOWN;
        };
    }

    private static boolean isJson(String typeName) {
        return typeName != null && typeName.toUpperCase(Locale.ROOT).contains("JSON");
    }

    private static boolean isUuid(String typeName) {
        return typeName != null && typeName.equalsIgnoreCase("UUID");
    }

    private static String dateString(Object value) {
        return value instanceof LocalDate localDate
                ? localDate.toString()
                : ((java.sql.Date) value).toLocalDate().toString();
    }

    private static String timeString(Object value) {
        return value instanceof LocalTime localTime
                ? localTime.toString()
                : ((java.sql.Time) value).toLocalTime().toString();
    }

    private static String offsetTimeString(Object value) {
        return value instanceof OffsetTime offsetTime ? offsetTime.toString() : value.toString();
    }

    private static String timestampString(Object value) {
        return value instanceof LocalDateTime localDateTime
                ? localDateTime.toString()
                : ((java.sql.Timestamp) value).toLocalDateTime().toString();
    }

    private static String offsetTimestampString(Object value) {
        return value instanceof OffsetDateTime offsetDateTime
                ? offsetDateTime.toString()
                : value.toString();
    }

    private static Object parseTime(String value) {
        return value.contains("+") || value.lastIndexOf('-') > 1
                ? OffsetTime.parse(value)
                : LocalTime.parse(value);
    }

    private static String scalar(String value) throws RuntimeFailure {
        String scalar = value == null ? "" : value;
        ProtocolLimits.requireUtf8(scalar, ProtocolLimits.MAX_SCALAR_BYTES, "column_metadata");
        return scalar;
    }

    private static void setMetadataString(
            java.util.function.Consumer<String> setter,
            String value,
            int acceptedEncodedBytes,
            JdbcColumn.Builder column,
            int maximumEncodedBytes)
            throws RuntimeFailure {
        setter.accept(metadataScalar(
                value,
                acceptedEncodedBytes,
                column,
                maximumEncodedBytes));
        ensureMetadataFits(
                acceptedEncodedBytes, column, maximumEncodedBytes);
    }

    private static void setOptionalMetadataString(
            java.util.function.Consumer<String> setter,
            String value,
            int acceptedEncodedBytes,
            JdbcColumn.Builder column,
            int maximumEncodedBytes)
            throws RuntimeFailure {
        if (value != null && !value.isEmpty()) {
            setMetadataString(
                    setter,
                    value,
                    acceptedEncodedBytes,
                    column,
                    maximumEncodedBytes);
        }
    }

    private static String metadataScalar(
            String value,
            int acceptedEncodedBytes,
            JdbcColumn.Builder column,
            int maximumEncodedBytes)
            throws RuntimeFailure {
        String scalar = value == null ? "" : value;
        int currentlyEncoded = acceptedEncodedBytes + encodedColumnSize(column.build());
        int remainingBytes = Math.max(0, maximumEncodedBytes - currentlyEncoded);
        int fieldLimit = Math.min(ProtocolLimits.MAX_SCALAR_BYTES, remainingBytes);
        if (ProtocolLimits.utf8LengthExceeds(scalar, fieldLimit)) {
            if (fieldLimit == ProtocolLimits.MAX_SCALAR_BYTES) {
                throw RuntimeFailure.limit("column_metadata", ProtocolLimits.MAX_SCALAR_BYTES);
            }
            throw RuntimeFailure.limit("query_metadata", maximumEncodedBytes);
        }
        return scalar;
    }

    private static void ensureMetadataFits(
            int acceptedEncodedBytes,
            JdbcColumn.Builder column,
            int maximumEncodedBytes)
            throws RuntimeFailure {
        if (acceptedEncodedBytes + encodedColumnSize(column.build()) > maximumEncodedBytes) {
            throw RuntimeFailure.limit("query_metadata", maximumEncodedBytes);
        }
    }

    private static int encodedColumnSize(JdbcColumn column) {
        return CodedOutputStream.computeMessageSize(1, column);
    }

    static final class ValueLimitExceeded extends Exception {
        private final String field;
        private final int maximumBytes;

        private ValueLimitExceeded(String field, int maximumBytes) {
            this.field = field;
            this.maximumBytes = maximumBytes;
        }

        String field() {
            return field;
        }

        int maximumBytes() {
            return maximumBytes;
        }
    }
}

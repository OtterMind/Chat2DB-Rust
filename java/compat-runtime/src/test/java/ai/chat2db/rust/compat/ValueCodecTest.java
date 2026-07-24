package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.JdbcColumn;
import ai.chat2db.rust.compat.protocol.v1.JdbcParameter;
import ai.chat2db.rust.compat.protocol.v1.JdbcValue;
import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.lang.reflect.Proxy;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLXML;
import java.sql.Types;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

class ValueCodecTest {

    private static final int LIMIT = 8;

    @Test
    void textReadsAtMostLimitPlusOneCharacters() {
        TrackingReader reader = new TrackingReader(128);
        assertBounded(Types.VARCHAR, "VARCHAR", "getCharacterStream", reader);
        assertTrue(reader.readCount() <= LIMIT + 1);
    }

    @Test
    void binaryReadsAtMostLimitPlusOneBytes() {
        TrackingInputStream input = new TrackingInputStream(128);
        assertBounded(Types.VARBINARY, "VARBINARY", "getBinaryStream", input);
        assertTrue(input.readCount() <= LIMIT + 1);
    }

    @Test
    void blobAndClobUseBoundedStreamsInsteadOfMaterializingAccessors() {
        TrackingInputStream binary = new TrackingInputStream(128);
        Blob blob = proxy(Blob.class, (method, arguments) -> switch (method) {
            case "getBinaryStream" -> binary;
            case "free" -> null;
            default -> throw new AssertionError("unexpected Blob accessor: " + method);
        });
        assertBounded(Types.BLOB, "BLOB", "getBlob", blob);
        assertTrue(binary.readCount() <= LIMIT + 1);

        TrackingReader text = new TrackingReader(128);
        Clob clob = proxy(Clob.class, (method, arguments) -> switch (method) {
            case "getCharacterStream" -> text;
            case "free" -> null;
            default -> throw new AssertionError("unexpected Clob accessor: " + method);
        });
        assertBounded(Types.CLOB, "CLOB", "getClob", clob);
        assertTrue(text.readCount() <= LIMIT + 1);
    }

    @Test
    void sqlXmlAndOpaqueValuesUseBoundedReaders() {
        TrackingReader xmlReader = new TrackingReader(128);
        SQLXML xml = proxy(SQLXML.class, (method, arguments) -> switch (method) {
            case "getCharacterStream" -> xmlReader;
            case "free" -> null;
            default -> throw new AssertionError("unexpected SQLXML accessor: " + method);
        });
        assertBounded(Types.SQLXML, "XML", "getSQLXML", xml);
        assertTrue(xmlReader.readCount() <= LIMIT + 1);

        TrackingReader opaqueReader = new TrackingReader(128);
        assertBounded(Types.OTHER, "CUSTOM", "getCharacterStream", opaqueReader);
        assertTrue(opaqueReader.readCount() <= LIMIT + 1);
    }

    @Test
    void exactTextLimitDoesNotProbePastEndIntoAnError() throws Exception {
        TrackingReader reader = new TrackingReader(LIMIT);
        ResultSet resultSet = resultSet("getCharacterStream", reader);
        var value = ValueCodec.read(resultSet, column(Types.VARCHAR, "VARCHAR"), LIMIT);
        assertEquals("xxxxxxxx", value.getTextValue());
        assertEquals(LIMIT, reader.readCount());
    }

    @Test
    void decimalUsesCanonicalScientificNotationAtExtremeScale() throws Exception {
        BigDecimal positiveScale = new GuardedBigDecimal(BigInteger.ONE, Integer.MAX_VALUE);
        BigDecimal negativeScale = new GuardedBigDecimal(BigInteger.ONE, Integer.MIN_VALUE);

        var tiny = ValueCodec.read(
                resultSet("getBigDecimal", positiveScale),
                column(Types.DECIMAL, "DECIMAL"),
                64);
        var huge = ValueCodec.read(
                resultSet("getBigDecimal", negativeScale),
                column(Types.DECIMAL, "DECIMAL"),
                64);

        assertEquals("1E-2147483647", tiny.getDecimalValue());
        assertEquals("1E+2147483648", huge.getDecimalValue());
    }

    @Test
    void temporalAndUuidBindingsCheckScalarLimitBeforeParsingOrJdbc() {
        String oversized = "x".repeat(ProtocolLimits.MAX_SCALAR_BYTES + 1);
        List<JdbcValue> values = List.of(
                JdbcValue.newBuilder().setDateValue(oversized).build(),
                JdbcValue.newBuilder().setTimeValue(oversized).build(),
                JdbcValue.newBuilder().setTimestampValue(oversized).build(),
                JdbcValue.newBuilder().setTimestampWithTimeZoneValue(oversized).build(),
                JdbcValue.newBuilder().setUuidValue(oversized).build());
        PreparedStatement statement = proxy(PreparedStatement.class, (method, arguments) -> {
            throw new AssertionError("JDBC must not receive an oversized scalar: " + method);
        });

        for (JdbcValue value : values) {
            JdbcParameter parameter = JdbcParameter.newBuilder()
                    .setPosition(1)
                    .setValue(value)
                    .build();
            RuntimeFailure failure = assertThrows(
                    RuntimeFailure.class, () -> ValueCodec.bind(statement, List.of(parameter)));
            assertEquals("protocol.limit_exceeded", failure.code());
        }
    }

    @Test
    void oversizedColumnMetadataStopsBeforeReadingLaterFields() {
        AtomicInteger typeNameReads = new AtomicInteger();
        AtomicInteger labelReads = new AtomicInteger();
        ResultSetMetaData metadata = metadata((method, arguments) -> switch (method) {
            case "getColumnCount" -> 1;
            case "getColumnType" -> Types.VARCHAR;
            case "isSigned" -> false;
            case "isNullable" -> ResultSetMetaData.columnNullable;
            case "getPrecision", "getScale", "getColumnDisplaySize" -> 0;
            case "getColumnTypeName" -> {
                typeNameReads.incrementAndGet();
                yield "x".repeat(1_000_000);
            }
            case "getColumnLabel" -> {
                labelReads.incrementAndGet();
                yield "unreachable";
            }
            default -> throw new AssertionError("unexpected metadata accessor: " + method);
        });

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> ValueCodec.columns(metadata, 512));
        assertEquals("protocol.limit_exceeded", failure.code());
        assertEquals(1, typeNameReads.get());
        assertEquals(0, labelReads.get());
    }

    @Test
    void cumulativeMetadataBudgetStopsAHostileColumnCountEarly() {
        AtomicInteger visitedColumns = new AtomicInteger();
        ResultSetMetaData metadata = metadata((method, arguments) -> switch (method) {
            case "getColumnCount" -> ProtocolLimits.MAX_COLUMNS;
            case "getColumnType" -> {
                visitedColumns.incrementAndGet();
                yield Types.VARCHAR;
            }
            case "isSigned" -> false;
            case "isNullable" -> ResultSetMetaData.columnNullable;
            case "getPrecision", "getScale" -> 0;
            case "getColumnDisplaySize" -> 200;
            case "getColumnTypeName" -> "VARCHAR";
            case "getColumnLabel", "getColumnName" -> "m".repeat(200);
            case "getCatalogName", "getSchemaName", "getTableName" -> "";
            default -> throw new AssertionError("unexpected metadata accessor: " + method);
        });

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> ValueCodec.columns(metadata, 800));
        assertEquals("protocol.limit_exceeded", failure.code());
        assertTrue(visitedColumns.get() >= 1);
        assertTrue(visitedColumns.get() <= 3, "metadata must stop near the negotiated budget");
    }

    private static void assertBounded(
            int jdbcType, String typeName, String accessor, Object suppliedValue) {
        ResultSet resultSet = resultSet(accessor, suppliedValue);
        assertThrows(
                ValueCodec.ValueLimitExceeded.class,
                () -> ValueCodec.read(resultSet, column(jdbcType, typeName), LIMIT));
    }

    private static JdbcColumn column(int jdbcType, String typeName) {
        return JdbcColumn.newBuilder()
                .setOrdinal(1)
                .setJdbcType(jdbcType)
                .setJdbcTypeName(typeName)
                .build();
    }

    private static ResultSet resultSet(String accessor, Object suppliedValue) {
        return proxy(ResultSet.class, (method, arguments) -> {
            if (method.equals(accessor)) {
                return suppliedValue;
            }
            if (method.equals("wasNull")) {
                return false;
            }
            throw new AssertionError("unexpected ResultSet accessor: " + method);
        });
    }

    private static ResultSetMetaData metadata(Invocation invocation) {
        return proxy(ResultSetMetaData.class, invocation);
    }

    @SuppressWarnings("unchecked")
    private static <T> T proxy(Class<T> type, Invocation invocation) {
        return (T) Proxy.newProxyInstance(
                ValueCodecTest.class.getClassLoader(),
                new Class<?>[] {type},
                (proxy, method, arguments) -> invocation.invoke(method.getName(), arguments));
    }

    @FunctionalInterface
    private interface Invocation {
        Object invoke(String method, Object[] arguments) throws Throwable;
    }

    private static final class TrackingInputStream extends InputStream {
        private int remaining;
        private int readCount;

        private TrackingInputStream(int length) {
            remaining = length;
        }

        @Override
        public int read() {
            if (remaining == 0) {
                return -1;
            }
            remaining--;
            readCount++;
            return 'x';
        }

        @Override
        public int read(byte[] buffer, int offset, int length) {
            if (remaining == 0) {
                return -1;
            }
            int count = Math.min(remaining, length);
            java.util.Arrays.fill(buffer, offset, offset + count, (byte) 'x');
            remaining -= count;
            readCount += count;
            return count;
        }

        private int readCount() {
            return readCount;
        }
    }

    private static final class TrackingReader extends Reader {
        private int remaining;
        private int readCount;

        private TrackingReader(int length) {
            remaining = length;
        }

        @Override
        public int read(char[] buffer, int offset, int length) {
            if (remaining == 0) {
                return -1;
            }
            int count = Math.min(remaining, length);
            java.util.Arrays.fill(buffer, offset, offset + count, 'x');
            remaining -= count;
            readCount += count;
            return count;
        }

        @Override
        public void close() throws IOException {
            // Nothing to release.
        }

        private int readCount() {
            return readCount;
        }
    }

    private static final class GuardedBigDecimal extends BigDecimal {
        private static final long serialVersionUID = 1L;

        private GuardedBigDecimal(BigInteger unscaledValue, int scale) {
            super(unscaledValue, scale);
        }

        @Override
        public String toPlainString() {
            throw new AssertionError("decimal encoding must not expand through toPlainString");
        }
    }
}

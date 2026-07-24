package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.JdbcCreditWindowLimit;
import ai.chat2db.rust.compat.protocol.v1.JdbcProtocolLimit;
import ai.chat2db.rust.compat.protocol.v1.JdbcResultByteLimit;

/** Accessors and validation helpers for the hard limits defined by jdbc.proto. */
final class ProtocolLimits {

    static final int MAX_ERROR_CAUSES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_ERROR_CAUSES);
    static final int MAX_DRIVER_ARTIFACTS = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_DRIVER_ARTIFACTS);
    static final int MAX_CREDIT_GRANT = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_CREDIT_GRANT);
    static final int MAX_CONNECTION_PROPERTIES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_CONNECTION_PROPERTIES);
    static final int MAX_DRIVER_ID_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_DRIVER_ID_BYTES);
    static final int MAX_PROPERTY_KEY_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_PROPERTY_KEY_BYTES);
    static final int MAX_DRIVER_CLASS_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_DRIVER_CLASS_BYTES);
    static final int MAX_OUTSTANDING_CREDITS =
            JdbcCreditWindowLimit.JDBC_CREDIT_WINDOW_LIMIT_MAX_OUTSTANDING_CREDITS.getNumber();
    static final int MAX_COLUMNS = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_COLUMNS);
    static final int MAX_BATCH_ROWS = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_BATCH_ROWS);
    static final int MAX_PATH_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_PATH_BYTES);
    static final int MAX_PARAMETERS = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_PARAMETERS);
    static final int MAX_JDBC_URL_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_JDBC_URL_BYTES);
    static final int MAX_PROPERTY_VALUE_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_PROPERTY_VALUE_BYTES);
    static final int MAX_SQL_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_SQL_BYTES);
    static final int MAX_SCALAR_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_SCALAR_BYTES);
    static final int MAX_BATCH_BYTES = value(JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_BATCH_BYTES);
    static final long DEFAULT_RESULT_BYTES = JdbcResultByteLimit
            .JDBC_RESULT_BYTE_LIMIT_DEFAULT_RESULT_BYTES
            .getNumber();
    static final long MAX_RESULT_BYTES =
            JdbcResultByteLimit.JDBC_RESULT_BYTE_LIMIT_MAX_RESULT_BYTES.getNumber();

    private ProtocolLimits() {
    }

    static int utf8Length(String value) {
        long bytes = 0;
        for (int offset = 0; offset < value.length(); ) {
            int codePoint = value.codePointAt(offset);
            bytes += codePoint <= 0x7f
                    ? 1
                    : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
            if (bytes >= Integer.MAX_VALUE) {
                return Integer.MAX_VALUE;
            }
            offset += Character.charCount(codePoint);
        }
        return (int) bytes;
    }

    static boolean utf8LengthExceeds(String value, int maximumBytes) {
        if (maximumBytes < 0) {
            return true;
        }
        int bytes = 0;
        for (int offset = 0; offset < value.length(); ) {
            int codePoint = value.codePointAt(offset);
            int encodedBytes = codePoint <= 0x7f
                    ? 1
                    : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
            if (bytes > maximumBytes - encodedBytes) {
                return true;
            }
            bytes += encodedBytes;
            offset += Character.charCount(codePoint);
        }
        return false;
    }

    static void requireUtf8(String value, int maximumBytes, String field) throws RuntimeFailure {
        if (utf8LengthExceeds(value, maximumBytes)) {
            throw RuntimeFailure.limit(field, maximumBytes);
        }
    }

    static void requireNonBlankUtf8(String value, int maximumBytes, String field)
            throws RuntimeFailure {
        if (value == null || value.isBlank()) {
            throw RuntimeFailure.validation("protocol.invalid_" + field, field + " is required");
        }
        requireUtf8(value, maximumBytes, field);
    }

    static String truncateUtf8(String value, int maximumBytes) {
        if (value == null || value.isEmpty()) {
            return value;
        }
        if (maximumBytes <= 0) {
            return "";
        }
        int bytes = 0;
        int end = 0;
        while (end < value.length()) {
            int codePoint = value.codePointAt(end);
            int encodedBytes = codePoint <= 0x7f
                    ? 1
                    : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
            if (bytes > maximumBytes - encodedBytes) {
                return value.substring(0, end);
            }
            bytes += encodedBytes;
            end += Character.charCount(codePoint);
        }
        return value;
    }

    private static int value(JdbcProtocolLimit limit) {
        return limit.getNumber();
    }
}

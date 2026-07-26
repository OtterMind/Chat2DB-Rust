package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityFormattedSql;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlFormatterLimit;
import com.github.vertical_blank.sqlformatter.SqlFormatter;
import com.github.vertical_blank.sqlformatter.languages.Dialect;
import java.util.Locale;
import java.util.Objects;

/** Bounded SQL formatting compatible with the Community service behavior. */
final class CommunitySqlFormatter {

    static final int MAX_RESPONSE_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES.getNumber();
    static final int MAX_COMPLEXITY_UNITS = CommunitySqlFormatterLimit
            .COMMUNITY_SQL_FORMATTER_LIMIT_MAX_COMPLEXITY_UNITS
            .getNumber();
    private static final int MAX_DATABASE_TYPE_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES.getNumber();

    private final FormattingEngine engine;

    CommunitySqlFormatter() {
        this((dialect, sql) -> dialect == null
                ? SqlFormatter.format(sql)
                : SqlFormatter.of(dialect).format(sql));
    }

    CommunitySqlFormatter(FormattingEngine engine) {
        this.engine = Objects.requireNonNull(engine, "engine");
    }

    CommunityFormattedSql format(String databaseType, String sql) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                databaseType, MAX_DATABASE_TYPE_BYTES, "database_type");
        ProtocolLimits.requireNonBlankUtf8(sql, ProtocolLimits.MAX_SQL_BYTES, "sql");
        requireFormatterComplexity(sql);

        String formatted = sql;
        try {
            formatted = engine.format(dialectFor(databaseType), sql);
        } catch (Exception ignored) {
            // Community returns the original SQL when the formatter rejects an input.
        }
        if (formatted == null) {
            throw RuntimeFailure.internal(
                    "community.sql_formatter_failed",
                    "the Community SQL formatter returned no SQL",
                    new IllegalStateException("SQL formatter returned null"));
        }

        ProtocolLimits.requireNonBlankUtf8(
                formatted, ProtocolLimits.MAX_SQL_BYTES, "formatted_sql");
        CommunityFormattedSql response =
                CommunityFormattedSql.newBuilder().setSql(formatted).build();
        requireResponseBudget(response.getSerializedSize());
        return response;
    }

    static Dialect dialectFor(String databaseType) {
        return switch (databaseType.toLowerCase(Locale.ROOT)) {
            case "mysql" -> Dialect.MySql;
            case "postgresql" -> Dialect.PostgreSql;
            case "oracle" -> Dialect.PlSql;
            case "sqlserver" -> Dialect.TSql;
            case "db2" -> Dialect.Db2;
            case "mariadb" -> Dialect.MariaDb;
            default -> null;
        };
    }

    static void requireFormatterComplexity(String sql) throws RuntimeFailure {
        int units = 0;
        boolean inAsciiWord = false;
        for (int offset = 0; offset < sql.length(); ) {
            int codePoint = sql.codePointAt(offset);
            offset += Character.charCount(codePoint);
            boolean asciiWord = codePoint >= 'a' && codePoint <= 'z'
                    || codePoint >= 'A' && codePoint <= 'Z'
                    || codePoint >= '0' && codePoint <= '9'
                    || codePoint == '_'
                    || codePoint == '$';
            if (asciiWord) {
                if (!inAsciiWord) {
                    units++;
                }
                inAsciiWord = true;
            } else {
                inAsciiWord = false;
                boolean asciiWhitespace = codePoint == ' '
                        || codePoint >= '\t' && codePoint <= '\r';
                if (!asciiWhitespace) {
                    units++;
                }
            }
            if (units > MAX_COMPLEXITY_UNITS) {
                throw RuntimeFailure.limit(
                        "Community SQL formatter complexity", MAX_COMPLEXITY_UNITS);
            }
        }
    }

    static void requireResponseBudget(int encodedBytes) throws RuntimeFailure {
        if (encodedBytes < 0 || encodedBytes > MAX_RESPONSE_BYTES) {
            throw RuntimeFailure.limit("Community formatted SQL response", MAX_RESPONSE_BYTES);
        }
    }

    @FunctionalInterface
    interface FormattingEngine {
        String format(Dialect dialect, String sql) throws Exception;
    }
}

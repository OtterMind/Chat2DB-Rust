package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTimeout;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.github.vertical_blank.sqlformatter.languages.Dialect;
import java.time.Duration;
import java.util.concurrent.atomic.AtomicBoolean;
import org.junit.jupiter.api.Test;

class CommunitySqlFormatterTest {

    @Test
    void mapsCommunityDatabaseTypesToTheOriginalFormatterDialects() {
        assertSame(Dialect.MySql, CommunitySqlFormatter.dialectFor("MYSQL"));
        assertSame(Dialect.PostgreSql, CommunitySqlFormatter.dialectFor("PostgreSQL"));
        assertSame(Dialect.PlSql, CommunitySqlFormatter.dialectFor("oracle"));
        assertSame(Dialect.TSql, CommunitySqlFormatter.dialectFor("sqlserver"));
        assertSame(Dialect.Db2, CommunitySqlFormatter.dialectFor("DB2"));
        assertSame(Dialect.MariaDb, CommunitySqlFormatter.dialectFor("MariaDB"));
        assertNull(CommunitySqlFormatter.dialectFor("H2"));
        assertNull(CommunitySqlFormatter.dialectFor("other"));
    }

    @Test
    void formatsUnknownAndH2DatabaseTypesWithTheGenericDialect() throws Exception {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter();
        String sql = "select id,name from users where id=1";

        String h2 = formatter.format("H2", sql).getSql();
        String other = formatter.format("custom", sql).getSql();

        assertNotEquals(sql, h2);
        assertEquals(h2, other);
        assertTrue(h2.contains("\n"));
        assertTrue(h2.contains("from\n  users"));
    }

    @Test
    void formatterExceptionReturnsTheOriginalSql() throws Exception {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter((dialect, sql) -> {
            throw new IllegalArgumentException("unsupported syntax");
        });
        String original = "select /* formatter fallback */ 1";

        assertEquals(original, formatter.format("MYSQL", original).getSql());
    }

    @Test
    void validatesDatabaseTypeAndSqlBeforeFormatting() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter();

        RuntimeFailure blankType = assertThrows(
                RuntimeFailure.class, () -> formatter.format("  ", "SELECT 1"));
        assertEquals("protocol.invalid_database_type", blankType.code());

        RuntimeFailure blankSql = assertThrows(
                RuntimeFailure.class, () -> formatter.format("H2", "\n\t"));
        assertEquals("protocol.invalid_sql", blankSql.code());

        RuntimeFailure oversizedSql = assertThrows(
                RuntimeFailure.class,
                () -> formatter.format(
                        "H2", "\u00e9".repeat(ProtocolLimits.MAX_SQL_BYTES / 2 + 1)));
        assertEquals("protocol.limit_exceeded", oversizedSql.code());
    }

    @Test
    void independentlyBoundsFormatterOutput() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter(
                (dialect, sql) -> "x".repeat(ProtocolLimits.MAX_SQL_BYTES + 1));

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> formatter.format("H2", "SELECT 1"));

        assertEquals("protocol.limit_exceeded", failure.code());
        assertTrue(failure.getMessage().contains("formatted_sql"));
    }

    @Test
    void rejectsTokenDenseSqlBeforeEnteringTheSuperlinearFormatter() throws Exception {
        AtomicBoolean called = new AtomicBoolean();
        CommunitySqlFormatter formatter = new CommunitySqlFormatter((dialect, sql) -> {
            called.set(true);
            return sql;
        });
        String exact = "a,".repeat(CommunitySqlFormatter.MAX_COMPLEXITY_UNITS / 2);

        assertEquals(exact, formatter.format("H2", exact).getSql());
        called.set(false);
        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> formatter.format("H2", exact + "a"));

        assertEquals("protocol.limit_exceeded", failure.code());
        assertTrue(failure.getMessage().contains("16384"));
        assertFalse(called.get(), "over-limit SQL must fail before the formatter is called");
    }

    @Test
    void acceptsOneMegabyteSqlWhenItsComplexityIsBounded() throws Exception {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter((dialect, sql) -> sql);
        String sql = "a".repeat(ProtocolLimits.MAX_SQL_BYTES);

        assertEquals(sql, formatter.format("H2", sql).getSql());
    }

    @Test
    void realFormatterRejectsTheReproducedTokenDenseInputBeforeItsDeadline() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter();
        String sql = "select " + "a,".repeat(150_000) + "z from t";

        assertTimeout(Duration.ofSeconds(2), () -> {
            RuntimeFailure failure =
                    assertThrows(RuntimeFailure.class, () -> formatter.format("H2", sql));
            assertEquals("protocol.limit_exceeded", failure.code());
            assertTrue(failure.getMessage().contains("16384"));
        });
    }

    @Test
    void realFormatterKeepsLargeLowComplexityInputBelowTheDeadline() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter();
        String sql = "select '" + "a".repeat(ProtocolLimits.MAX_SQL_BYTES - 1_024) + "'";

        assertTimeout(Duration.ofSeconds(5), () -> {
            String formatted = formatter.format("H2", sql).getSql();
            assertTrue(formatted.startsWith("select"));
        });
    }

    @Test
    void rejectsBlankFormatterOutput() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter((dialect, sql) -> "  ");

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> formatter.format("H2", "SELECT 1"));

        assertEquals("protocol.invalid_formatted_sql", failure.code());
    }

    @Test
    void enforcesTheIndependentCommunityResponseBudget() throws Exception {
        CommunitySqlFormatter.requireResponseBudget(CommunitySqlFormatter.MAX_RESPONSE_BYTES);

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class,
                () -> CommunitySqlFormatter.requireResponseBudget(
                        CommunitySqlFormatter.MAX_RESPONSE_BYTES + 1));

        assertEquals("protocol.limit_exceeded", failure.code());
        assertTrue(failure.getMessage().contains("8388608"));
    }

    @Test
    void nullFormatterOutputIsAnInternalFailure() {
        CommunitySqlFormatter formatter = new CommunitySqlFormatter((dialect, sql) -> null);

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class, () -> formatter.format("H2", "SELECT 1"));

        assertEquals("community.sql_formatter_failed", failure.code());
    }
}

package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityTablePreviewSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltTablePreviewSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDqlRowLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityNamespaceByteLimit;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

/** Bounded reflective bridge to the retained Community DQL builder. */
final class CommunityDqlBuilder {

    private static final String PAGE_LIMIT_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.PageLimitRequest";
    private static final int MAX_DATABASE_TYPE_BYTES = CommunityByteLimit
            .COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES
            .getNumber();
    private static final int MAX_IDENTIFIER_BYTES = CommunityNamespaceByteLimit
            .COMMUNITY_NAMESPACE_BYTE_LIMIT_MAX_IDENTIFIER_BYTES
            .getNumber();
    private static final int MAX_ROW_LIMIT =
            CommunityDqlRowLimit.COMMUNITY_DQL_ROW_LIMIT_MAX_ROWS.getNumber();
    private static final int MAX_RESPONSE_BYTES = CommunityByteLimit
            .COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES
            .getNumber();
    private static final int MAX_QUALIFIED_IDENTIFIER_BYTES = MAX_IDENTIFIER_BYTES * 3 + 8;

    private final ClassLoader loader;

    CommunityDqlBuilder(ClassLoader loader) {
        this.loader = loader;
    }

    CommunityBuiltTablePreviewSql build(
            Object plugin, BuildCommunityTablePreviewSqlRequest request)
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

    CommunityBuiltTablePreviewSql build(
            Dialect dialect, BuildCommunityTablePreviewSqlRequest request)
            throws ReflectiveOperationException, RuntimeFailure {
        validateRequest(request);
        List<String> segments = new ArrayList<>(3);
        if (!request.getDatabaseName().isEmpty()) {
            segments.add(request.getDatabaseName());
        }
        if (!request.getSchemaName().isEmpty()) {
            segments.add(request.getSchemaName());
        }
        segments.add(request.getTableName());

        String qualifiedIdentifier = dialect.quoteQualifiedIdentifier(
                segments.toArray(String[]::new));
        ProtocolLimits.requireNonBlankUtf8(
                qualifiedIdentifier,
                MAX_QUALIFIED_IDENTIFIER_BYTES,
                "quoted_table_identifier");
        requireSafeRenderedSql(qualifiedIdentifier, "quoted table identifier");

        String selectSql = buildSelectTable(
                dialect, "", "", qualifiedIdentifier);
        if (!selectSql.contains(qualifiedIdentifier)) {
            selectSql = buildSelectTable(
                    dialect,
                    request.getDatabaseName(),
                    request.getSchemaName(),
                    request.getTableName());
            if (!selectSql.contains(qualifiedIdentifier)) {
                throw incompatible();
            }
        }

        String limitedSql = dialect.buildPageLimit(selectSql, request.getRowLimit());
        ProtocolLimits.requireNonBlankUtf8(
                limitedSql, ProtocolLimits.MAX_SQL_BYTES, "table_preview_sql");
        requireSafeRenderedSql(limitedSql, "table preview SQL");

        CommunityBuiltTablePreviewSql response = CommunityBuiltTablePreviewSql.newBuilder()
                .setSql(limitedSql)
                .setRowLimit(request.getRowLimit())
                .build();
        if (response.getSerializedSize() > MAX_RESPONSE_BYTES) {
            throw RuntimeFailure.limit("Community table preview response", MAX_RESPONSE_BYTES);
        }
        return response;
    }

    static void validateRequest(BuildCommunityTablePreviewSqlRequest request)
            throws RuntimeFailure {
        if (request == null) {
            throw RuntimeFailure.validation(
                    "community.dql_request_required",
                    "the Community DQL request is required");
        }
        ProtocolLimits.requireNonBlankUtf8(
                request.getDatabaseType(), MAX_DATABASE_TYPE_BYTES, "database_type");
        requireIdentifier(request.getDatabaseName(), true, "database_name");
        requireIdentifier(request.getSchemaName(), true, "schema_name");
        requireIdentifier(request.getTableName(), false, "table_name");
        int rowLimit = request.getRowLimit();
        if (rowLimit < 1 || rowLimit > MAX_ROW_LIMIT) {
            throw RuntimeFailure.validation(
                    "community.dql_row_limit_invalid",
                    "the Community DQL row limit must be between 1 and " + MAX_ROW_LIMIT);
        }
    }

    private static void requireIdentifier(String value, boolean optional, String field)
            throws RuntimeFailure {
        String present = value == null ? "" : value;
        if (optional && present.isEmpty()) {
            return;
        }
        ProtocolLimits.requireNonBlankUtf8(present, MAX_IDENTIFIER_BYTES, field);
        if (!present.strip().equals(present)
                || hasControl(present)
                || containsAny(present, '.', ';', '\'', '"', '`', '[', ']')
                || present.contains("--")
                || present.contains("/*")
                || present.contains("*/")) {
            throw RuntimeFailure.validation(
                    "community.dql_identifier_invalid",
                    "a Community DQL identifier contains unsafe syntax");
        }
    }

    private static void requireSafeRenderedSql(String value, String field)
            throws RuntimeFailure {
        if (hasControlExceptWhitespace(value) || value.indexOf('\0') >= 0) {
            throw RuntimeFailure.validation(
                    "community.dql_rendered_sql_invalid",
                    "the Community " + field + " contains unsafe control characters");
        }
    }

    private static String buildSelectTable(
            Dialect dialect,
            String databaseName,
            String schemaName,
            String tableName)
            throws ReflectiveOperationException, RuntimeFailure {
        String selectSql = dialect.buildSelectTable(databaseName, schemaName, tableName);
        ProtocolLimits.requireNonBlankUtf8(
                selectSql, ProtocolLimits.MAX_SQL_BYTES, "table_preview_select_sql");
        requireSafeRenderedSql(selectSql, "table preview SELECT SQL");
        return selectSql;
    }

    private static boolean hasControl(String value) {
        return value.codePoints().anyMatch(Character::isISOControl);
    }

    private static boolean containsAny(String value, char... candidates) {
        for (char candidate : candidates) {
            if (value.indexOf(candidate) >= 0) {
                return true;
            }
        }
        return false;
    }

    private static boolean hasControlExceptWhitespace(String value) {
        return value.codePoints().anyMatch(codePoint -> Character.isISOControl(codePoint)
                && codePoint != '\t'
                && codePoint != '\n'
                && codePoint != '\r');
    }

    private static RuntimeFailure notSupported() {
        return RuntimeFailure.validation(
                "community.dql_builder_not_supported",
                "the selected Community plugin does not support table preview SQL generation");
    }

    private static RuntimeFailure incompatible() {
        return RuntimeFailure.validation(
                "community.dql_builder_incompatible",
                "the selected Community plugin produced incompatible table preview SQL");
    }

    private static RuntimeFailure failed(Throwable cause) {
        return RuntimeFailure.internal(
                "community.dql_builder_failed",
                "the Community DQL builder failed internally",
                cause);
    }

    private static Throwable invocationCause(InvocationTargetException failure) {
        return failure.getCause() == null ? failure : failure.getCause();
    }

    interface Dialect {
        String quoteQualifiedIdentifier(String[] identifiers)
                throws ReflectiveOperationException;

        String buildSelectTable(String databaseName, String schemaName, String tableName)
                throws ReflectiveOperationException;

        String buildPageLimit(String sql, int rowLimit)
                throws ReflectiveOperationException;
    }

    private static final class ReflectiveDialect implements Dialect {
        private final ClassLoader loader;
        private final Object identifierBuilder;
        private final Object dqlBuilder;

        private ReflectiveDialect(ClassLoader loader, Object plugin)
                throws ReflectiveOperationException {
            if (plugin == null) {
                throw new UnsupportedOperationException("Community plugin is unavailable");
            }
            Object sqlBuilder = invoke(plugin, "getSqlBuilder");
            identifierBuilder = sqlBuilder == null ? null : invoke(sqlBuilder, "identifier");
            dqlBuilder = sqlBuilder == null ? null : invoke(sqlBuilder, "dql");
            if (identifierBuilder == null || dqlBuilder == null) {
                throw new UnsupportedOperationException("Community DQL components are unavailable");
            }
            this.loader = loader;
        }

        @Override
        public String quoteQualifiedIdentifier(String[] identifiers)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    identifierBuilder,
                    "quoteQualifiedIdentifier",
                    new Class<?>[] {String[].class},
                    (Object) identifiers));
        }

        @Override
        public String buildSelectTable(
                String databaseName, String schemaName, String tableName)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    dqlBuilder,
                    "buildSelectTable",
                    new Class<?>[] {String.class, String.class, String.class},
                    databaseName,
                    schemaName,
                    tableName));
        }

        @Override
        public String buildPageLimit(String sql, int rowLimit)
                throws ReflectiveOperationException {
            Class<?> requestType = Class.forName(PAGE_LIMIT_REQUEST_CLASS, true, loader);
            Object request = requestType.getDeclaredConstructor().newInstance();
            invokeSetter(request, "setSql", String.class, sql);
            invokeSetter(request, "setOffset", int.class, 0);
            invokeSetter(request, "setPageNo", int.class, 1);
            invokeSetter(request, "setPageSize", int.class, rowLimit);
            return stringResult(invoke(
                    dqlBuilder,
                    "buildPageLimit",
                    new Class<?>[] {requestType},
                    request));
        }
    }

    private static Object invoke(Object target, String method)
            throws ReflectiveOperationException {
        return invoke(target, method, new Class<?>[0]);
    }

    private static Object invoke(
            Object target, String method, Class<?>[] parameterTypes, Object... arguments)
            throws ReflectiveOperationException {
        if (target == null) {
            throw new UnsupportedOperationException("Community DQL component is unavailable");
        }
        Method reflected = target.getClass().getMethod(method, parameterTypes);
        return reflected.invoke(target, arguments);
    }

    private static void invokeSetter(
            Object target, String method, Class<?> parameterType, Object value)
            throws ReflectiveOperationException {
        invoke(target, method, new Class<?>[] {parameterType}, value);
    }

    private static String stringResult(Object value) {
        return value == null ? "" : value.toString();
    }
}

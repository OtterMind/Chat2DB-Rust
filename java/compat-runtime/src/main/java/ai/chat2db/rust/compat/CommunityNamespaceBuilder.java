package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityNamespaceSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityAlterDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityAlterSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltNamespaceSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabase;
import ai.chat2db.rust.compat.protocol.v1.CommunityNamespaceByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchema;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;

/** Bounded reflective bridge to the retained Community database and schema builders. */
final class CommunityNamespaceBuilder {

    private static final String DATABASE_CLASS =
            "ai.chat2db.community.domain.api.model.metadata.Database";
    private static final String SCHEMA_CLASS =
            "ai.chat2db.community.domain.api.model.metadata.Schema";
    private static final int MAX_DATABASE_TYPE_BYTES = CommunityByteLimit
            .COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES
            .getNumber();
    private static final int MAX_IDENTIFIER_BYTES = CommunityNamespaceByteLimit
            .COMMUNITY_NAMESPACE_BYTE_LIMIT_MAX_IDENTIFIER_BYTES
            .getNumber();
    private static final int MAX_PROPERTY_BYTES = CommunityNamespaceByteLimit
            .COMMUNITY_NAMESPACE_BYTE_LIMIT_MAX_PROPERTY_BYTES
            .getNumber();
    private static final int MAX_COMMENT_BYTES = CommunityByteLimit
            .COMMUNITY_BYTE_LIMIT_MAX_COMMENT_BYTES
            .getNumber();
    private static final int MAX_RESPONSE_BYTES = CommunityByteLimit
            .COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES
            .getNumber();

    private final ClassLoader loader;

    CommunityNamespaceBuilder(ClassLoader loader) {
        this.loader = loader;
    }

    CommunityBuiltNamespaceSql build(
            Object plugin, BuildCommunityNamespaceSqlRequest request)
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

    CommunityBuiltNamespaceSql build(
            Dialect dialect, BuildCommunityNamespaceSqlRequest request)
            throws ReflectiveOperationException, RuntimeFailure {
        validateRequest(request);
        String sql = switch (request.getOperationCase()) {
            case CREATE_DATABASE -> createDatabase(dialect, request.getCreateDatabase());
            case ALTER_DATABASE -> alterDatabase(dialect, request.getAlterDatabase());
            case DROP_DATABASE -> dialect.buildDropDatabase(requireIdentifier(
                    request.getDropDatabase().getDatabaseName(), "database_name"));
            case USE_DATABASE -> dialect.buildUseDatabase(requireIdentifier(
                    request.getUseDatabase().getDatabaseName(), "database_name"));
            case CREATE_SCHEMA -> createSchema(dialect, request.getCreateSchema());
            case ALTER_SCHEMA -> alterSchema(dialect, request.getAlterSchema());
            case DROP_SCHEMA -> dialect.buildDropSchema(requireIdentifier(
                    request.getDropSchema().getSchemaName(), "schema_name"));
            case OPERATION_NOT_SET -> throw operationRequired();
        };
        ProtocolLimits.requireNonBlankUtf8(
                sql, ProtocolLimits.MAX_SQL_BYTES, "built_namespace_sql");
        CommunityBuiltNamespaceSql response =
                CommunityBuiltNamespaceSql.newBuilder().setSql(sql).build();
        if (response.getSerializedSize() > MAX_RESPONSE_BYTES) {
            throw RuntimeFailure.limit("Community namespace response", MAX_RESPONSE_BYTES);
        }
        return response;
    }

    static void validateRequest(BuildCommunityNamespaceSqlRequest request)
            throws RuntimeFailure {
        if (request == null) {
            throw RuntimeFailure.validation(
                    "community.namespace_request_required",
                    "the Community namespace request is required");
        }
        ProtocolLimits.requireNonBlankUtf8(
                request.getDatabaseType(), MAX_DATABASE_TYPE_BYTES, "database_type");
        if (request.getOperationCase()
                == BuildCommunityNamespaceSqlRequest.OperationCase.OPERATION_NOT_SET) {
            throw operationRequired();
        }
    }

    private static String createDatabase(
            Dialect dialect, CommunityCreateDatabaseSql operation)
            throws ReflectiveOperationException, RuntimeFailure {
        if (!operation.hasDatabase()) {
            throw RuntimeFailure.validation(
                    "community.namespace_database_required",
                    "a Community database definition is required");
        }
        return dialect.buildCreateDatabase(database(operation.getDatabase(), "database"));
    }

    private static String alterDatabase(
            Dialect dialect, CommunityAlterDatabaseSql operation)
            throws ReflectiveOperationException, RuntimeFailure {
        if (!operation.hasOldDatabase() || !operation.hasNewDatabase()) {
            throw RuntimeFailure.validation(
                    "community.namespace_database_required",
                    "both old and new Community database definitions are required");
        }
        DatabaseSpec oldDatabase = database(operation.getOldDatabase(), "old_database");
        DatabaseSpec newDatabase = database(operation.getNewDatabase(), "new_database");
        return dialect.buildAlterDatabase(oldDatabase, newDatabase);
    }

    private static String createSchema(
            Dialect dialect, CommunityCreateSchemaSql operation)
            throws ReflectiveOperationException, RuntimeFailure {
        if (!operation.hasSchema()) {
            throw RuntimeFailure.validation(
                    "community.namespace_schema_required",
                    "a Community schema definition is required");
        }
        return dialect.buildCreateSchema(schema(operation.getSchema()));
    }

    private static String alterSchema(
            Dialect dialect, CommunityAlterSchemaSql operation)
            throws ReflectiveOperationException, RuntimeFailure {
        String oldName = requireIdentifier(operation.getOldSchemaName(), "old_schema_name");
        String newName = requireIdentifier(operation.getNewSchemaName(), "new_schema_name");
        return dialect.buildAlterSchema(oldName, newName);
    }

    private static DatabaseSpec database(CommunityDatabase requested, String fieldPrefix)
            throws RuntimeFailure {
        return new DatabaseSpec(
                requireIdentifier(requested.getName(), fieldPrefix + "_name"),
                requireComment(requested.getComment(), fieldPrefix + "_comment"),
                requireProperty(requested.getCharset(), fieldPrefix + "_charset"),
                requireProperty(
                        requested.getCollation(),
                        fieldPrefix + "_collation"),
                requireProperty(requested.getOwner(), fieldPrefix + "_owner"),
                requested.getSystem());
    }

    private static SchemaSpec schema(CommunitySchema requested) throws RuntimeFailure {
        return new SchemaSpec(
                requireOptionalIdentifier(requested.getDatabaseName(), "schema_database_name"),
                requireIdentifier(requested.getName(), "schema_name"),
                requireComment(requested.getComment(), "schema_comment"),
                requireProperty(requested.getOwner(), "schema_owner"),
                requested.getSystem());
    }

    private static String requireIdentifier(String value, String field) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(value, MAX_IDENTIFIER_BYTES, field);
        if (!value.strip().equals(value)
                || hasControl(value)
                || containsAny(value, '.', ';', '\'', '"', '`', '[', ']')
                || containsCommentMarker(value)) {
            throw RuntimeFailure.validation(
                    "community.namespace_identifier_invalid",
                    "a Community namespace identifier contains unsafe syntax");
        }
        return value;
    }

    private static String requireOptionalIdentifier(String value, String field)
            throws RuntimeFailure {
        if (value == null || value.isEmpty()) {
            return "";
        }
        return requireIdentifier(value, field);
    }

    private static String requireUtf8(String value, int maximum, String field)
            throws RuntimeFailure {
        String present = value == null ? "" : value;
        ProtocolLimits.requireUtf8(present, maximum, field);
        return present;
    }

    private static String requireProperty(String value, String field) throws RuntimeFailure {
        String present = requireUtf8(value, MAX_PROPERTY_BYTES, field);
        if (!present.isEmpty()
                && (!present.strip().equals(present)
                        || !present.codePoints().allMatch(CommunityNamespaceBuilder::isPropertyCodePoint)
                        || containsCommentMarker(present))) {
            throw RuntimeFailure.validation(
                    "community.namespace_property_invalid",
                    "a Community namespace property contains unsafe syntax");
        }
        return present;
    }

    private static String requireComment(String value, String field) throws RuntimeFailure {
        String present = requireUtf8(value, MAX_COMMENT_BYTES, field);
        if (!present.isEmpty()
                && (hasControl(present)
                        || containsAny(present, '\'', '\\')
                        || containsCommentMarker(present))) {
            throw RuntimeFailure.validation(
                    "community.namespace_comment_invalid",
                    "a Community namespace comment contains unsafe syntax");
        }
        return present;
    }

    private static boolean isPropertyCodePoint(int codePoint) {
        return Character.isLetterOrDigit(codePoint)
                || codePoint == '_'
                || codePoint == '-'
                || codePoint == '$'
                || codePoint == '@';
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

    private static boolean containsCommentMarker(String value) {
        return value.contains("--") || value.contains("/*") || value.contains("*/");
    }

    private static RuntimeFailure operationRequired() {
        return RuntimeFailure.validation(
                "community.namespace_operation_required",
                "a Community namespace operation is required");
    }

    private static RuntimeFailure notSupported() {
        return RuntimeFailure.validation(
                "community.namespace_builder_not_supported",
                "the selected Community plugin does not support this namespace operation");
    }

    private static RuntimeFailure failed(Throwable cause) {
        return RuntimeFailure.internal(
                "community.namespace_builder_failed",
                "the Community namespace builder failed internally",
                cause);
    }

    private static Throwable invocationCause(InvocationTargetException failure) {
        return failure.getCause() == null ? failure : failure.getCause();
    }

    interface Dialect {
        String buildCreateDatabase(DatabaseSpec database) throws ReflectiveOperationException;

        String buildAlterDatabase(DatabaseSpec oldDatabase, DatabaseSpec newDatabase)
                throws ReflectiveOperationException;

        String buildDropDatabase(String databaseName) throws ReflectiveOperationException;

        String buildUseDatabase(String databaseName) throws ReflectiveOperationException;

        String buildCreateSchema(SchemaSpec schema) throws ReflectiveOperationException;

        String buildAlterSchema(String oldSchemaName, String newSchemaName)
                throws ReflectiveOperationException;

        String buildDropSchema(String schemaName) throws ReflectiveOperationException;
    }

    record DatabaseSpec(
            String name,
            String comment,
            String charset,
            String collation,
            String owner,
            boolean system) {}

    record SchemaSpec(
            String databaseName, String name, String comment, String owner, boolean system) {}

    private static final class ReflectiveDialect implements Dialect {
        private final ClassLoader loader;
        private final Object ddlBuilder;

        private ReflectiveDialect(ClassLoader loader, Object plugin)
                throws ReflectiveOperationException {
            if (plugin == null) {
                throw new UnsupportedOperationException("Community plugin is unavailable");
            }
            Object metadata = invoke(plugin, "getDbMetaData");
            Object sqlBuilder = metadata == null ? null : invoke(metadata, "getSqlBuilder");
            ddlBuilder = sqlBuilder == null ? null : invoke(sqlBuilder, "ddl");
            if (ddlBuilder == null) {
                throw new UnsupportedOperationException(
                        "Community namespace builder is unavailable");
            }
            this.loader = loader;
        }

        @Override
        public String buildCreateDatabase(DatabaseSpec database)
                throws ReflectiveOperationException {
            Class<?> type = Class.forName(DATABASE_CLASS, true, loader);
            return stringResult(invoke(
                    databaseBuilder(),
                    "buildCreateDatabase",
                    new Class<?>[] {type},
                    communityDatabase(type, database)));
        }

        @Override
        public String buildAlterDatabase(DatabaseSpec oldDatabase, DatabaseSpec newDatabase)
                throws ReflectiveOperationException {
            Class<?> type = Class.forName(DATABASE_CLASS, true, loader);
            return stringResult(invoke(
                    databaseBuilder(),
                    "buildAlterDatabase",
                    new Class<?>[] {type, type},
                    communityDatabase(type, oldDatabase),
                    communityDatabase(type, newDatabase)));
        }

        @Override
        public String buildDropDatabase(String databaseName)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    databaseBuilder(),
                    "buildDropDatabase",
                    new Class<?>[] {String.class},
                    databaseName));
        }

        @Override
        public String buildUseDatabase(String databaseName)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    databaseBuilder(),
                    "buildUseDatabase",
                    new Class<?>[] {String.class},
                    databaseName));
        }

        @Override
        public String buildCreateSchema(SchemaSpec schema)
                throws ReflectiveOperationException {
            Class<?> type = Class.forName(SCHEMA_CLASS, true, loader);
            return stringResult(invoke(
                    schemaBuilder(),
                    "buildCreateSchema",
                    new Class<?>[] {type},
                    communitySchema(type, schema)));
        }

        @Override
        public String buildAlterSchema(String oldSchemaName, String newSchemaName)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    schemaBuilder(),
                    "buildAlterSchema",
                    new Class<?>[] {String.class, String.class},
                    oldSchemaName,
                    newSchemaName));
        }

        @Override
        public String buildDropSchema(String schemaName)
                throws ReflectiveOperationException {
            return stringResult(invoke(
                    schemaBuilder(),
                    "buildDropSchema",
                    new Class<?>[] {String.class},
                    schemaName));
        }

        private Object databaseBuilder() throws ReflectiveOperationException {
            Object builder = invoke(ddlBuilder, "database");
            if (builder == null) {
                throw new UnsupportedOperationException(
                        "Community database builder is unavailable");
            }
            return builder;
        }

        private Object schemaBuilder() throws ReflectiveOperationException {
            Object builder = invoke(ddlBuilder, "schema");
            if (builder == null) {
                throw new UnsupportedOperationException(
                        "Community schema builder is unavailable");
            }
            return builder;
        }

        private static Object communityDatabase(Class<?> type, DatabaseSpec database)
                throws ReflectiveOperationException {
            Object value = type.getDeclaredConstructor().newInstance();
            invokeSetter(value, "setName", String.class, database.name());
            invokeSetter(value, "setComment", String.class, database.comment());
            invokeSetter(value, "setCharset", String.class, database.charset());
            invokeSetter(value, "setCollation", String.class, database.collation());
            invokeSetter(value, "setOwner", String.class, database.owner());
            invokeSetter(value, "setSystem", boolean.class, database.system());
            return value;
        }

        private static Object communitySchema(Class<?> type, SchemaSpec schema)
                throws ReflectiveOperationException {
            Object value = type.getDeclaredConstructor().newInstance();
            invokeSetter(value, "setDatabaseName", String.class, schema.databaseName());
            invokeSetter(value, "setName", String.class, schema.name());
            invokeSetter(value, "setComment", String.class, schema.comment());
            invokeSetter(value, "setOwner", String.class, schema.owner());
            invokeSetter(value, "setSystem", boolean.class, schema.system());
            return value;
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
            throw new UnsupportedOperationException("Community namespace component is unavailable");
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

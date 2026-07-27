package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityNamespaceSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityAlterDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityAlterSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabase;
import ai.chat2db.rust.compat.protocol.v1.CommunityDropDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityDropSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchema;
import ai.chat2db.rust.compat.protocol.v1.CommunityUseDatabaseSql;
import org.junit.jupiter.api.Test;

class CommunityNamespaceBuilderTest {

    private final CommunityNamespaceBuilder builder =
            new CommunityNamespaceBuilder(getClass().getClassLoader());
    private final RecordingDialect dialect = new RecordingDialect();

    @Test
    void mapsEveryTypedOperationWithoutOpeningAConnection() throws Exception {
        CommunityDatabase database = CommunityDatabase.newBuilder()
                .setName("analytics")
                .setComment("reporting")
                .setCharset("utf8mb4")
                .setCollation("utf8mb4_bin")
                .setOwner("data_team")
                .setSystem(true)
                .build();
        assertEquals(
                "create-db:analytics:reporting:utf8mb4:utf8mb4_bin:data_team:true",
                builder.build(
                                dialect,
                                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                        .setDatabase(database))
                                        .build())
                        .getSql());

        assertEquals(
                "alter-db:old_db:new_db",
                builder.build(
                                dialect,
                                request().setAlterDatabase(CommunityAlterDatabaseSql.newBuilder()
                                        .setOldDatabase(database.toBuilder().setName("old_db"))
                                        .setNewDatabase(database.toBuilder().setName("new_db")))
                                        .build())
                        .getSql());
        assertEquals(
                "drop-db:archive",
                builder.build(
                                dialect,
                                request().setDropDatabase(CommunityDropDatabaseSql.newBuilder()
                                        .setDatabaseName("archive"))
                                        .build())
                        .getSql());
        assertEquals(
                "use-db:warehouse",
                builder.build(
                                dialect,
                                request().setUseDatabase(CommunityUseDatabaseSql.newBuilder()
                                        .setDatabaseName("warehouse"))
                                        .build())
                        .getSql());

        CommunitySchema schema = CommunitySchema.newBuilder()
                .setDatabaseName("warehouse")
                .setName("reporting")
                .setComment("curated")
                .setOwner("analyst")
                .setSystem(true)
                .build();
        assertEquals(
                "create-schema:warehouse:reporting:curated:analyst:true",
                builder.build(
                                dialect,
                                request().setCreateSchema(CommunityCreateSchemaSql.newBuilder()
                                        .setSchema(schema))
                                        .build())
                        .getSql());
        assertEquals(
                "alter-schema:before:after",
                builder.build(
                                dialect,
                                request().setAlterSchema(CommunityAlterSchemaSql.newBuilder()
                                        .setOldSchemaName("before")
                                        .setNewSchemaName("after"))
                                        .build())
                        .getSql());
        assertEquals(
                "drop-schema:obsolete",
                builder.build(
                                dialect,
                                request().setDropSchema(CommunityDropSchemaSql.newBuilder()
                                        .setSchemaName("obsolete"))
                                        .build())
                        .getSql());
    }

    @Test
    void validatesOneofAndRequiredNestedDefinitions() {
        assertCode("community.namespace_request_required", () -> builder.build(dialect, null));
        assertCode(
                "protocol.invalid_database_type",
                () -> builder.build(
                        dialect,
                        BuildCommunityNamespaceSqlRequest.newBuilder()
                                .setDropSchema(CommunityDropSchemaSql.newBuilder()
                                        .setSchemaName("app"))
                                .build()));
        assertCode(
                "community.namespace_operation_required",
                () -> builder.build(dialect, request().build()));
        assertCode(
                "community.namespace_database_required",
                () -> builder.build(
                        dialect,
                        request().setCreateDatabase(
                                        CommunityCreateDatabaseSql.getDefaultInstance())
                                .build()));
        assertCode(
                "community.namespace_database_required",
                () -> builder.build(
                        dialect,
                        request().setAlterDatabase(
                                        CommunityAlterDatabaseSql.getDefaultInstance())
                                .build()));
        assertCode(
                "community.namespace_schema_required",
                () -> builder.build(
                        dialect,
                        request().setCreateSchema(
                                        CommunityCreateSchemaSql.getDefaultInstance())
                                .build()));
    }

    @Test
    void enforcesUtf8IdentifierPropertyAndCommentBoundaries() throws Exception {
        BuildCommunityNamespaceSqlRequest exactDatabaseType = request()
                .setDatabaseType("x".repeat(128))
                .setDropSchema(CommunityDropSchemaSql.newBuilder().setSchemaName("app"))
                .build();
        assertEquals("drop-schema:app", builder.build(dialect, exactDatabaseType).getSql());
        assertLimit(() -> builder.build(
                dialect, exactDatabaseType.toBuilder().setDatabaseType("x".repeat(129)).build()));

        String identifierBoundary = "\u00e9".repeat(256);
        assertEquals(
                "drop-db:" + identifierBoundary,
                builder.build(
                                dialect,
                                request().setDropDatabase(CommunityDropDatabaseSql.newBuilder()
                                        .setDatabaseName(identifierBoundary))
                                        .build())
                        .getSql());
        assertLimit(() -> builder.build(
                dialect,
                request().setDropDatabase(CommunityDropDatabaseSql.newBuilder()
                                .setDatabaseName("\u00e9".repeat(257)))
                        .build()));

        CommunityDatabase boundary = CommunityDatabase.newBuilder()
                .setName("analytics")
                .setCharset("\u00e9".repeat(2048))
                .setCollation("x".repeat(4096))
                .setOwner("owner")
                .setComment("\u00e9".repeat(32768))
                .build();
        builder.build(
                dialect,
                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                .setDatabase(boundary))
                        .build());

        assertLimit(() -> builder.build(
                dialect,
                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                .setDatabase(boundary.toBuilder()
                                        .setCharset("\u00e9".repeat(2049))))
                        .build()));
        assertLimit(() -> builder.build(
                dialect,
                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                .setDatabase(boundary.toBuilder()
                                        .setCollation("x".repeat(4097))))
                        .build()));
        assertLimit(() -> builder.build(
                dialect,
                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                .setDatabase(boundary.toBuilder()
                                        .setOwner("x".repeat(4097))))
                        .build()));
        assertLimit(() -> builder.build(
                dialect,
                request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                .setDatabase(boundary.toBuilder()
                                        .setComment("\u00e9".repeat(32769))))
                        .build()));
        assertCode(
                "protocol.invalid_old_schema_name",
                () -> builder.build(
                        dialect,
                        request().setAlterSchema(CommunityAlterSchemaSql.newBuilder()
                                        .setOldSchemaName(" ")
                                        .setNewSchemaName("after"))
                                .build()));
    }

    @Test
    void rejectsOversizedOrEmptyBuilderOutput() {
        CommunityNamespaceBuilder.Dialect oversized =
                new DelegatingDialect(dialect) {
                    @Override
                    public String buildDropSchema(String schemaName) {
                        return "x".repeat(ProtocolLimits.MAX_SQL_BYTES + 1);
                    }
                };
        assertLimit(() -> builder.build(
                oversized,
                request().setDropSchema(CommunityDropSchemaSql.newBuilder()
                                .setSchemaName("app"))
                        .build()));

        CommunityNamespaceBuilder.Dialect empty =
                new DelegatingDialect(dialect) {
                    @Override
                    public String buildDropSchema(String schemaName) {
                        return "";
                    }
                };
        assertCode(
                "protocol.invalid_built_namespace_sql",
                () -> builder.build(
                        empty,
                        request().setDropSchema(CommunityDropSchemaSql.newBuilder()
                                        .setSchemaName("app"))
                                .build()));
    }

    @Test
    void rejectsUnsafeIdentifierAndPropertySyntaxWithoutEchoingInput() {
        for (String unsafe : new String[] {
            "catalog.schema", "catalog;drop", "catalog--comment", " catalog", "catalog\n"
        }) {
            RuntimeFailure failure = assertFailure(() -> builder.build(
                    dialect,
                    request().setUseDatabase(CommunityUseDatabaseSql.newBuilder()
                                    .setDatabaseName(unsafe))
                            .build()));
            assertEquals("community.namespace_identifier_invalid", failure.code());
            assertFalse(failure.getMessage().contains(unsafe));
        }

        for (String unsafe : new String[] {
            "utf8;drop", "owner'role", "owner/*comment", "owner--role", " owner", "owner\n",
            "owner role", "owner=role"
        }) {
            RuntimeFailure failure = assertFailure(() -> builder.build(
                    dialect,
                    request().setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                    .setDatabase(CommunityDatabase.newBuilder()
                                            .setName("analytics")
                                            .setOwner(unsafe)))
                            .build()));
            assertEquals("community.namespace_property_invalid", failure.code());
            assertFalse(failure.getMessage().contains(unsafe));
        }

        for (String unsafe : new String[] {
            "curated'; DROP SCHEMA app; --", "curated/*comment", "curated\\value",
            "curated\nvalue"
        }) {
            RuntimeFailure failure = assertFailure(() -> builder.build(
                    dialect,
                    request().setCreateSchema(CommunityCreateSchemaSql.newBuilder()
                                    .setSchema(CommunitySchema.newBuilder()
                                            .setDatabaseName("analytics")
                                            .setName("reporting")
                                            .setComment(unsafe)))
                            .build()));
            assertEquals("community.namespace_comment_invalid", failure.code());
            assertFalse(failure.getMessage().contains(unsafe));
        }
    }

    @Test
    void mapsSpiFailuresWithoutLeakingReflectionDetailsAndRestoresTccl() {
        ClassLoader previous = Thread.currentThread().getContextClassLoader();
        RuntimeFailure unsupported = assertFailure(() -> builder.build(
                new UnsupportedPlugin(),
                request().setDropDatabase(CommunityDropDatabaseSql.newBuilder()
                                .setDatabaseName("archive"))
                        .build()));
        assertEquals("community.namespace_builder_not_supported", unsupported.code());
        assertFalse(unsupported.getMessage().contains("unsupported-sensitive-detail"));
        assertSame(previous, Thread.currentThread().getContextClassLoader());

        RuntimeFailure failed = assertFailure(() -> builder.build(
                new FailingPlugin(),
                request().setDropDatabase(CommunityDropDatabaseSql.newBuilder()
                                .setDatabaseName("archive"))
                        .build()));
        assertEquals("community.namespace_builder_failed", failed.code());
        assertFalse(failed.getMessage().contains("reflection-sensitive-detail"));
        assertSame(previous, Thread.currentThread().getContextClassLoader());
    }

    private static BuildCommunityNamespaceSqlRequest.Builder request() {
        return BuildCommunityNamespaceSqlRequest.newBuilder().setDatabaseType("H2");
    }

    private static void assertLimit(ThrowingAction action) {
        assertCode("protocol.limit_exceeded", action);
    }

    private static void assertCode(String code, ThrowingAction action) {
        assertEquals(code, assertFailure(action).code());
    }

    private static RuntimeFailure assertFailure(ThrowingAction action) {
        return assertThrows(RuntimeFailure.class, action::run);
    }

    @FunctionalInterface
    private interface ThrowingAction {
        void run() throws Exception;
    }

    private static final class RecordingDialect implements CommunityNamespaceBuilder.Dialect {
        @Override
        public String buildCreateDatabase(CommunityNamespaceBuilder.DatabaseSpec database) {
            return "create-db:"
                    + database.name()
                    + ":"
                    + database.comment()
                    + ":"
                    + database.charset()
                    + ":"
                    + database.collation()
                    + ":"
                    + database.owner()
                    + ":"
                    + database.system();
        }

        @Override
        public String buildAlterDatabase(
                CommunityNamespaceBuilder.DatabaseSpec oldDatabase,
                CommunityNamespaceBuilder.DatabaseSpec newDatabase) {
            return "alter-db:" + oldDatabase.name() + ":" + newDatabase.name();
        }

        @Override
        public String buildDropDatabase(String databaseName) {
            return "drop-db:" + databaseName;
        }

        @Override
        public String buildUseDatabase(String databaseName) {
            return "use-db:" + databaseName;
        }

        @Override
        public String buildCreateSchema(CommunityNamespaceBuilder.SchemaSpec schema) {
            return "create-schema:"
                    + schema.databaseName()
                    + ":"
                    + schema.name()
                    + ":"
                    + schema.comment()
                    + ":"
                    + schema.owner()
                    + ":"
                    + schema.system();
        }

        @Override
        public String buildAlterSchema(String oldSchemaName, String newSchemaName) {
            return "alter-schema:" + oldSchemaName + ":" + newSchemaName;
        }

        @Override
        public String buildDropSchema(String schemaName) {
            return "drop-schema:" + schemaName;
        }
    }

    private static class DelegatingDialect implements CommunityNamespaceBuilder.Dialect {
        private final CommunityNamespaceBuilder.Dialect delegate;

        private DelegatingDialect(CommunityNamespaceBuilder.Dialect delegate) {
            this.delegate = delegate;
        }

        @Override
        public String buildCreateDatabase(CommunityNamespaceBuilder.DatabaseSpec database)
                throws ReflectiveOperationException {
            return delegate.buildCreateDatabase(database);
        }

        @Override
        public String buildAlterDatabase(
                CommunityNamespaceBuilder.DatabaseSpec oldDatabase,
                CommunityNamespaceBuilder.DatabaseSpec newDatabase)
                throws ReflectiveOperationException {
            return delegate.buildAlterDatabase(oldDatabase, newDatabase);
        }

        @Override
        public String buildDropDatabase(String databaseName)
                throws ReflectiveOperationException {
            return delegate.buildDropDatabase(databaseName);
        }

        @Override
        public String buildUseDatabase(String databaseName)
                throws ReflectiveOperationException {
            return delegate.buildUseDatabase(databaseName);
        }

        @Override
        public String buildCreateSchema(CommunityNamespaceBuilder.SchemaSpec schema)
                throws ReflectiveOperationException {
            return delegate.buildCreateSchema(schema);
        }

        @Override
        public String buildAlterSchema(String oldSchemaName, String newSchemaName)
                throws ReflectiveOperationException {
            return delegate.buildAlterSchema(oldSchemaName, newSchemaName);
        }

        @Override
        public String buildDropSchema(String schemaName)
                throws ReflectiveOperationException {
            return delegate.buildDropSchema(schemaName);
        }
    }

    public static final class UnsupportedPlugin {
        public Object getDbMetaData() {
            return new UnsupportedMetadata();
        }
    }

    public static final class UnsupportedMetadata {
        public Object getSqlBuilder() {
            return new UnsupportedSqlBuilder();
        }
    }

    public static final class UnsupportedSqlBuilder {
        public Object ddl() {
            return new UnsupportedDdlBuilder();
        }
    }

    public static final class UnsupportedDdlBuilder {
        public Object database() {
            throw new UnsupportedOperationException("unsupported-sensitive-detail");
        }
    }

    public static final class FailingPlugin {
        public Object getDbMetaData() {
            throw new IllegalStateException("reflection-sensitive-detail");
        }
    }
}

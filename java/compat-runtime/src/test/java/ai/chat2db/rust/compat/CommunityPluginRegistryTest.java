package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityDmlRequest;
import ai.chat2db.rust.compat.protocol.v1.BuildCommunityNamespaceSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.BuildCommunityTablePreviewSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityAlterDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateDatabaseSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabase;
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
import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKey;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKey;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchema;
import ai.chat2db.rust.compat.protocol.v1.CompleteCommunitySqlRequest;
import com.google.protobuf.ByteString;
import java.io.OutputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.Driver;
import java.sql.Statement;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.jar.Attributes;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import java.util.jar.Manifest;
import javax.tools.JavaCompiler;
import javax.tools.ToolProvider;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.function.Executable;
import org.junit.jupiter.api.io.TempDir;

class CommunityPluginRegistryTest {

    private static final int MIB = 1024 * 1024;
    private static final int LENGTH_DELIMITED_OVERHEAD = 6;

    @Test
    void rejectsManifestClassPathBeforeTheLoaderCanReachAnExternalJar(
            @TempDir Path temporaryDirectory) throws Exception {
        Path externalJar = temporaryDirectory.resolve("outside.jar");
        writeResourceJar(externalJar, "outside-only.txt", "outside");

        Path snapshot = Files.createDirectory(temporaryDirectory.resolve("snapshot"));
        Path classpathJar = snapshot.resolve("community.jar");
        Manifest manifest = new Manifest();
        manifest.getMainAttributes().put(Attributes.Name.MANIFEST_VERSION, "1.0");
        manifest.getMainAttributes().put(Attributes.Name.CLASS_PATH, "../outside.jar");
        try (OutputStream output = Files.newOutputStream(classpathJar);
                JarOutputStream ignored = new JarOutputStream(output, manifest)) {
            // The manifest points outside the validated snapshot.
        }

        try (URLClassLoader unsafeLoader = new URLClassLoader(
                new java.net.URL[] {classpathJar.toUri().toURL()},
                ClassLoader.getPlatformClassLoader())) {
            assertNotNull(
                    unsafeLoader.findResource("outside-only.txt"),
                    "the JDK loader follows manifest Class-Path entries");
        }

        IllegalStateException failure = assertThrows(
                IllegalStateException.class,
                () -> CommunityPluginRegistry.validateClasspath(
                        snapshot.toRealPath().toString()));
        assertTrue(failure.getMessage().contains("Class-Path"));
    }

    @Test
    void responseProjectionBudgetAcceptsTheBoundaryAndRejectsTheNextField()
            throws Exception {
        CommunityPluginRegistry.ProjectionBudget budget =
                CommunityPluginRegistry.ProjectionBudget.response();
        String oneMibEncodedField = "x".repeat(MIB - LENGTH_DELIMITED_OVERHEAD);

        for (int index = 0; index < 8; index++) {
            budget.consumeUtf8(oneMibEncodedField);
        }

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class,
                () -> budget.consumeUtf8(""));
        assertEquals("protocol.limit_exceeded", failure.code());
        assertTrue(failure.getMessage().contains("8388608"));
    }

    @Test
    void responseProjectionBudgetCountsUtf8BytesRatherThanUtf16Units() throws Exception {
        CommunityPluginRegistry.ProjectionBudget budget =
                CommunityPluginRegistry.ProjectionBudget.response();
        String twoByteCharacters = "\u00e9".repeat(
                (CommunityPluginRegistry.MAX_RESPONSE_PROJECTION_BYTES
                                - LENGTH_DELIMITED_OVERHEAD)
                        / 2);

        budget.consumeUtf8(twoByteCharacters);

        RuntimeFailure failure = assertThrows(
                RuntimeFailure.class,
                () -> budget.consumeUtf8("x"));
        assertEquals("protocol.limit_exceeded", failure.code());
    }

    @Test
    void sqlValidationRejectsExcessDiagnosticsAndNegativeCoordinates() {
        RuntimeFailure tooManyDiagnostics = assertThrows(
                RuntimeFailure.class,
                () -> CommunityPluginRegistry.requireSqlDiagnosticCount(
                        CommunityPluginRegistry.MAX_SQL_DIAGNOSTICS + 1));
        assertEquals("protocol.limit_exceeded", tooManyDiagnostics.code());
        assertEquals(
                0,
                CommunityPluginRegistry.requireNonNegativeCoordinate(
                        0, "diagnostic_start_line"));
        assertThrows(
                IllegalStateException.class,
                () -> CommunityPluginRegistry.requireNonNegativeCoordinate(
                        -1, "diagnostic_start_line"));
    }

    @Test
    void retainedSqlParserOperationsAreSerialized() throws Exception {
        Method parse = CommunityPluginRegistry.class.getDeclaredMethod(
                "parse", String.class, String.class);
        Method validate = CommunityPluginRegistry.class.getDeclaredMethod(
                "validate", String.class, String.class);

        assertTrue(Modifier.isSynchronized(parse.getModifiers()));
        assertTrue(Modifier.isSynchronized(validate.getModifiers()));
    }

    @Test
    void realCommunityH2SpiProjectsViewsForeignKeysAndPrimaryKeys() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (URLClassLoader driverLoader = new URLClassLoader(
                        new URL[] {h2DriverJar().toUri().toURL()},
                        ClassLoader.getPlatformClassLoader());
                CommunityPluginRegistry registry = openRegistry(communityClasspath);
                Connection connection = h2Connection(driverLoader)) {
            try (Statement statement = connection.createStatement()) {
                statement.executeUpdate(
                        "CREATE TABLE parent_table ("
                                + "parent_id BIGINT NOT NULL, "
                                + "CONSTRAINT pk_parent PRIMARY KEY (parent_id))");
                statement.executeUpdate(
                        "CREATE TABLE child_table ("
                                + "child_id BIGINT NOT NULL, parent_id BIGINT NOT NULL, "
                                + "CONSTRAINT pk_child PRIMARY KEY (child_id), "
                                + "CONSTRAINT fk_child_parent FOREIGN KEY (parent_id) "
                                + "REFERENCES parent_table(parent_id))");
                statement.executeUpdate(
                        "CREATE VIEW parent_child_view AS "
                                + "SELECT child.child_id, parent.parent_id "
                                + "FROM child_table child JOIN parent_table parent "
                                + "ON child.parent_id = parent.parent_id");
            }

            String databaseName = connection.getCatalog();
            assertTrue(registry
                    .views("H2", connection, databaseName, "PUBLIC", "PARENT_CHILD_VIEW")
                    .getViewsList()
                    .stream()
                    .anyMatch(view -> view.getName().equalsIgnoreCase("parent_child_view")));

            CommunityForeignKey imported = registry
                    .importedKeys("H2", connection, databaseName, "PUBLIC", "CHILD_TABLE")
                    .getKeysList()
                    .stream()
                    .filter(key -> key.getForeignKeyName().equalsIgnoreCase("fk_child_parent"))
                    .findFirst()
                    .orElseThrow();
            assertEquals("PARENT_TABLE", imported.getPrimaryTableName());
            assertEquals("PARENT_ID", imported.getPrimaryColumnName());
            assertEquals("CHILD_TABLE", imported.getForeignTableName());
            assertEquals("PARENT_ID", imported.getForeignColumnName());
            assertEquals("PK_PARENT", imported.getPrimaryKeyName());
            assertEquals(1, imported.getKeySequence());

            CommunityForeignKey exported = registry
                    .exportedKeys("H2", connection, databaseName, "PUBLIC", "PARENT_TABLE")
                    .getKeysList()
                    .stream()
                    .filter(key -> key.getForeignKeyName().equalsIgnoreCase("fk_child_parent"))
                    .findFirst()
                    .orElseThrow();
            assertEquals(imported, exported);

            CommunityPrimaryKey primaryKey = registry
                    .primaryKeys("H2", connection, databaseName, "PUBLIC", "PARENT_TABLE")
                    .getKeysList()
                    .stream()
                    .filter(key -> key.getName().equalsIgnoreCase("pk_parent"))
                    .findFirst()
                    .orElseThrow();
            assertEquals("PARENT_TABLE", primaryKey.getTableName());
            assertEquals("PARENT_ID", primaryKey.getColumnName());
        }
    }

    @Test
    void realCommunityH2SpiProjectsProgrammabilityMetadata(
            @TempDir Path temporaryDirectory) throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (URLClassLoader driverLoader = h2DriverWithTrigger(temporaryDirectory);
                CommunityPluginRegistry registry = openRegistry(communityClasspath);
                Connection connection = h2Connection(driverLoader)) {
            try (Statement statement = connection.createStatement()) {
                statement.executeUpdate(
                        "CREATE ALIAS add_one AS "
                                + "'int addOne(int value) { return value + 1; }'");
                statement.executeUpdate(
                        "CREATE ALIAS record_event AS "
                                + "'void recordEvent(int value) { }'");
                statement.executeUpdate(
                        "CREATE TABLE programmability_events (event_id BIGINT PRIMARY KEY)");
                statement.executeUpdate(
                        "CREATE TRIGGER audit_trigger BEFORE INSERT ON programmability_events "
                                + "CALL 'ai.chat2db.rust.compat.fixture.AuditTrigger'");
            }

            String databaseName = connection.getCatalog();
            String schemaName = "PUBLIC";
            // H2 exposes Java aliases only through JDBC procedure metadata, even when the
            // information schema classifies an alias as a function.
            assertEquals(
                    0,
                    registry.functions("H2", connection, databaseName, schemaName)
                            .getFunctionsCount());
            var function = registry.function(
                    "H2", connection, databaseName, schemaName, "ADD_ONE");
            assertEquals(databaseName, function.getDatabaseName());
            assertEquals(schemaName, function.getSchemaName());
            assertEquals("ADD_ONE", function.getName());
            assertTrue(function.getBody().contains("addOne"));
            assertEquals(
                    0,
                    registry.functionParameters(
                                    "H2", connection, databaseName, schemaName, "ADD_ONE")
                            .getParametersCount());

            assertTrue(registry
                    .procedures("H2", connection, databaseName, schemaName)
                    .getProceduresList()
                    .stream()
                    .anyMatch(procedure -> procedure.getName().equalsIgnoreCase("record_event")));
            var procedure = registry.procedure(
                    "H2", connection, databaseName, schemaName, "RECORD_EVENT");
            assertEquals(databaseName, procedure.getDatabaseName());
            assertEquals(schemaName, procedure.getSchemaName());
            assertEquals("RECORD_EVENT", procedure.getName());
            assertTrue(procedure.getBody().contains("recordEvent"));
            assertTrue(registry
                    .procedureParameters(
                            "H2", connection, databaseName, schemaName, "RECORD_EVENT")
                    .getParametersList()
                    .stream()
                    .anyMatch(parameter ->
                            parameter.getProcedureName().equalsIgnoreCase("record_event")));

            assertTrue(registry
                    .triggers("H2", connection, databaseName, schemaName)
                    .getTriggersList()
                    .stream()
                    .anyMatch(trigger -> trigger.getName().equalsIgnoreCase("audit_trigger")));
            var trigger = registry.trigger(
                    "H2", connection, databaseName, schemaName, "AUDIT_TRIGGER");
            assertEquals(databaseName, trigger.getDatabaseName());
            assertEquals(schemaName, trigger.getSchemaName());
            assertEquals("AUDIT_TRIGGER", trigger.getName());
            assertTrue(trigger.getBody().contains("AuditTrigger"));

            assertFailureCode(
                    "community.function_not_found",
                    () -> registry.function(
                            "H2", connection, databaseName, schemaName, "MISSING_FUNCTION"));
            assertFailureCode(
                    "community.procedure_not_found",
                    () -> registry.procedure(
                            "H2", connection, databaseName, schemaName, "MISSING_PROCEDURE"));
            assertFailureCode(
                    "community.trigger_not_found",
                    () -> registry.trigger(
                            "H2", connection, databaseName, schemaName, "MISSING_TRIGGER"));

            assertFailureCode(
                    "community.function_not_found",
                    () -> registry.function(
                            "H2",
                            connection,
                            databaseName,
                            schemaName,
                            "ADD_ONE' OR '1'='1"));
            assertFailureCode(
                    "community.procedure_not_found",
                    () -> registry.procedure(
                            "H2",
                            connection,
                            databaseName,
                            schemaName,
                            "RECORD_EVENT' OR '1'='1"));
            assertFailureCode(
                    "community.trigger_not_found",
                    () -> registry.trigger(
                            "H2",
                            connection,
                            databaseName,
                            schemaName,
                            "AUDIT_TRIGGER' OR '1'='1"));
            assertEquals(
                    0,
                    registry.triggers(
                                    "H2",
                                    connection,
                                    databaseName,
                                    "PUBLIC'; DROP TABLE programmability_events; --")
                            .getTriggersCount());
            try (var sentinelStatement = connection.createStatement();
                    var result = sentinelStatement.executeQuery(
                            "SELECT COUNT(*) FROM programmability_events")) {
                assertTrue(result.next(), "the injection sentinel table must still exist");
            }

            assertFailureCode(
                    "community.catalog_mismatch",
                    () -> registry.function(
                            "H2", connection, "WRONG_CATALOG", schemaName, "ADD_ONE"));
            assertFailureCode(
                    "community.catalog_mismatch",
                    () -> registry.procedure(
                            "H2", connection, "WRONG_CATALOG", schemaName, "RECORD_EVENT"));
            assertFailureCode(
                    "community.catalog_mismatch",
                    () -> registry.trigger(
                            "H2", connection, "WRONG_CATALOG", schemaName, "AUDIT_TRIGGER"));
        }
    }

    @Test
    void realCommunityH2SqlToolsFormatAndValidateSql() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            var formatted = new CommunitySqlFormatter()
                    .format("H2", "select id,name from items where id=1");
            assertTrue(formatted.getSql().contains("from\n  items"));

            var valid = registry.validate("H2", "SELECT 1;");
            assertTrue(valid.getValid());
            assertFalse(valid.getStatementsList().isEmpty());
            assertTrue(valid.getDiagnosticsList().isEmpty());

            var invalid = registry.validate("H2", "SELECT FROM;");
            assertFalse(invalid.getValid());
            assertFalse(invalid.getStatementsList().isEmpty());
            assertFalse(invalid.getDiagnosticsList().isEmpty());
            var diagnostic = invalid.getDiagnostics(0);
            assertTrue(diagnostic.getStartLine() > 0);
            assertTrue(diagnostic.getEndLine() >= diagnostic.getStartLine());
            assertTrue(diagnostic.getEndColumn() >= diagnostic.getStartColumn());
            assertFalse(diagnostic.getMessage().isBlank());
        }
    }

    @Test
    void realCommunityH2SqlCompletionClearsRequestStateAndKeepsTheConnectionOpen()
            throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (URLClassLoader driverLoader = new URLClassLoader(
                        new URL[] {h2DriverJar().toUri().toURL()},
                        ClassLoader.getPlatformClassLoader());
                CommunityPluginRegistry registry = openRegistry(communityClasspath);
                Connection connection = h2Connection(driverLoader)) {
            try (Statement statement = connection.createStatement()) {
                statement.executeUpdate("CREATE SCHEMA IF NOT EXISTS APP");
                statement.executeUpdate(
                        "CREATE TABLE APP.completion_users "
                                + "(id BIGINT PRIMARY KEY, display_name VARCHAR(64))");
            }

            String databaseName = connection.getCatalog();
            ClassLoader communityLoader = registryLoader(registry);
            long tableScope = 7_001L;
            String adjacentCacheKey =
                    "databases_datasourceId_70010_schemaName_APP_tables";
            String unrelatedCacheKey = "custom_datasourceId_7001_value";
            putCommunityCache(communityLoader, adjacentCacheKey, "adjacent");
            putCommunityCache(communityLoader, unrelatedCacheKey, "unrelated");

            ClassLoader previous = Thread.currentThread().getContextClassLoader();
            var tables = registry.completeSql(
                    connection,
                    completionRequest(
                            databaseName,
                            "select * from ",
                            "select * from ".length(),
                            tableScope));
            assertEquals(previous, Thread.currentThread().getContextClassLoader());
            assertEquals("SUCCESS", tables.getStatus());
            assertTrue(tables.getCandidatesList().stream().anyMatch(candidate ->
                    candidate.getLabel().equalsIgnoreCase("completion_users")
                            && candidate.getType().equals("TABLE")));
            assertCompletionStateCleared(communityLoader, tableScope);
            assertEquals("adjacent", getCommunityCache(communityLoader, adjacentCacheKey));
            assertEquals("unrelated", getCommunityCache(communityLoader, unrelatedCacheKey));

            long columnScope = tableScope + 1;
            String columnSql = "select completion_users. from APP.completion_users";
            var columns = registry.completeSql(
                    connection,
                    completionRequest(
                            databaseName,
                            columnSql,
                            "select completion_users.".length(),
                            columnScope));
            assertEquals("SUCCESS", columns.getStatus());
            for (String expected : List.of("id", "display_name")) {
                assertTrue(columns.getCandidatesList().stream().anyMatch(candidate ->
                        candidate.getLabel().equalsIgnoreCase(expected)
                                && candidate.getType().equals("COLUMN")));
            }
            assertCompletionStateCleared(communityLoader, columnScope);
            assertFalse(connection.isClosed());
            try (Statement statement = connection.createStatement()) {
                assertTrue(statement.execute("SELECT 1"));
            }

            long failureScope = tableScope + 2;
            try (Connection closedConnection = h2Connection(driverLoader)) {
                String closedDatabaseName = closedConnection.getCatalog();
                closedConnection.close();
                RuntimeFailure failure = assertThrows(
                        RuntimeFailure.class,
                        () -> registry.completeSql(
                                closedConnection,
                                completionRequest(
                                        closedDatabaseName,
                                        "select * from ",
                                        "select * from ".length(),
                                        failureScope)));
                assertEquals("community.sql_completion_connection_closed", failure.code());
            }
            assertEquals(previous, Thread.currentThread().getContextClassLoader());
            assertCompletionStateCleared(communityLoader, failureScope);

            removeCommunityCache(communityLoader, adjacentCacheKey);
            removeCommunityCache(communityLoader, unrelatedCacheKey);
        }
    }

    private static URLClassLoader h2DriverWithTrigger(Path temporaryDirectory)
            throws Exception {
        Path h2Jar = h2DriverJar();
        Path source = temporaryDirectory
                .resolve("src/ai/chat2db/rust/compat/fixture/AuditTrigger.java");
        Files.createDirectories(source.getParent());
        Files.writeString(
                source,
                "package ai.chat2db.rust.compat.fixture;\n"
                        + "public final class AuditTrigger implements org.h2.api.Trigger {\n"
                        + "  public void fire(java.sql.Connection connection, "
                        + "Object[] oldRow, Object[] newRow) { }\n"
                        + "}\n",
                StandardCharsets.UTF_8);
        Path classes = Files.createDirectory(temporaryDirectory.resolve("classes"));
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        assertNotNull(compiler, "tests require a full JDK");
        assertEquals(
                0,
                compiler.run(
                        null,
                        null,
                        null,
                        "-classpath",
                        h2Jar.toString(),
                        "-d",
                        classes.toString(),
                        source.toString()));
        return new URLClassLoader(
                new URL[] {h2Jar.toUri().toURL(), classes.toUri().toURL()},
                ClassLoader.getPlatformClassLoader());
    }

    @Test
    void nullProgrammabilityDetailsReturnStableNotFoundErrors() {
        assertMissingDetail("community.function_not_found", "function");
        assertMissingDetail("community.procedure_not_found", "procedure");
        assertMissingDetail("community.trigger_not_found", "trigger");
    }

    @Test
    void realCommunityH2BuildsNamespaceSqlWithoutOpeningJdbc() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(hasPlugin(registry, "H2"), "the fixed classpath does not contain H2");
            CommunitySchema schema = CommunitySchema.newBuilder()
                    .setDatabaseName("local")
                    .setName("rust_namespace")
                    .setComment("generated only")
                    .setOwner("owner")
                    .build();
            BuildCommunityNamespaceSqlRequest request =
                    BuildCommunityNamespaceSqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setCreateSchema(CommunityCreateSchemaSql.newBuilder()
                                    .setSchema(schema))
                            .build();

            ClassLoader previous = Thread.currentThread().getContextClassLoader();
            assertEquals(
                    "CREATE SCHEMA \"rust_namespace\";\nCOMMENT ON SCHEMA \"rust_namespace\""
                            + " IS 'generated only';",
                    registry.buildNamespace(request).getSql());
            assertEquals(previous, Thread.currentThread().getContextClassLoader());
            assertEquals(
                    registry.buildNamespace(request).getSql(),
                    registry.buildCreateSchema("H2", schema).getSql());
        }
    }

    @Test
    void realCommunityMysqlBuildsDatabaseNamespaceSql() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    hasPlugin(registry, "MYSQL"),
                    "the fixed classpath does not contain the MySQL plugin");
            CommunityDatabase database = CommunityDatabase.newBuilder()
                    .setName("analytics")
                    .setCharset("utf8mb4")
                    .setCollation("utf8mb4_0900_ai_ci")
                    .build();
            assertEquals(
                    "CREATE DATABASE `analytics` DEFAULT CHARACTER SET=utf8mb4"
                            + " COLLATE=utf8mb4_0900_ai_ci",
                    registry.buildNamespace(BuildCommunityNamespaceSqlRequest.newBuilder()
                                    .setDatabaseType("MYSQL")
                                    .setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                            .setDatabase(database))
                                    .build())
                            .getSql());
        }
    }

    @Test
    void realCommunityPostgresqlBuildsSchemaNamespaceSql() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    hasPlugin(registry, "POSTGRESQL"),
                    "the fixed classpath does not contain the PostgreSQL plugin");
            CommunitySchema schema = CommunitySchema.newBuilder()
                    .setName("reporting")
                    .setOwner("analyst")
                    .setComment("curated")
                    .build();
            assertEquals(
                    "CREATE SCHEMA \"reporting\" AUTHORIZATION analyst;"
                            + " COMMENT ON SCHEMA \"reporting\" IS 'curated';",
                    registry.buildNamespace(BuildCommunityNamespaceSqlRequest.newBuilder()
                                    .setDatabaseType("POSTGRESQL")
                                    .setCreateSchema(CommunityCreateSchemaSql.newBuilder()
                                            .setSchema(schema))
                                    .build())
                            .getSql());
        }
    }

    @Test
    void realCommunityNamespaceMapsUnsupportedAndRejectsOversizedInput() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(hasPlugin(registry, "H2"), "the fixed classpath does not contain H2");
            CommunityDatabase database = CommunityDatabase.newBuilder()
                    .setName("before")
                    .build();
            assertFailureCode(
                    "community.namespace_builder_not_supported",
                    () -> registry.buildNamespace(BuildCommunityNamespaceSqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setAlterDatabase(CommunityAlterDatabaseSql.newBuilder()
                                    .setOldDatabase(database)
                                    .setNewDatabase(database.toBuilder().setName("after")))
                            .build()));

            RuntimeFailure identifier = assertFailureCode(
                    "protocol.limit_exceeded",
                    () -> registry.buildNamespace(BuildCommunityNamespaceSqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                    .setDatabase(database.toBuilder().setName("x".repeat(513))))
                            .build()));
            assertFalse(identifier.getMessage().contains("x".repeat(513)));

            RuntimeFailure property = assertFailureCode(
                    "protocol.limit_exceeded",
                    () -> registry.buildNamespace(BuildCommunityNamespaceSqlRequest.newBuilder()
                            .setDatabaseType("H2")
                            .setCreateDatabase(CommunityCreateDatabaseSql.newBuilder()
                                    .setDatabase(database.toBuilder()
                                            .setCharset("x".repeat(4097))))
                            .build()));
            assertFalse(property.getMessage().contains("x".repeat(4097)));
        }
    }

    @Test
    void realCommunityH2BuildsAndExecutesBoundedDml() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (URLClassLoader driverLoader = new URLClassLoader(
                        new URL[] {h2DriverJar().toUri().toURL()},
                        ClassLoader.getPlatformClassLoader());
                CommunityPluginRegistry registry = openRegistry(communityClasspath);
                Connection connection = h2Connection(driverLoader)) {
            var descriptor = registry.catalog().getPluginsList().stream()
                    .filter(plugin -> plugin.getDatabaseType().equalsIgnoreCase("H2"))
                    .findFirst()
                    .orElseThrow();
            assertTrue(descriptor.getDmlBuilderAvailable());
            assertTrue(descriptor.getValueProcessorAvailable());
            assertTrue(descriptor.getIdentifierProcessorAvailable());
            assertTrue(descriptor.getDqlBuilderAvailable());

            try (Statement statement = connection.createStatement()) {
                statement.executeUpdate("CREATE SCHEMA IF NOT EXISTS APP");
                statement.executeUpdate(
                        "CREATE TABLE APP.items ("
                                + "id BIGINT PRIMARY KEY, label VARCHAR(128), active BOOLEAN, "
                                + "created_at TIMESTAMP, note VARCHAR(128), payload VARBINARY)");
            }

            BuildCommunityDmlRequest single = dmlRequest(CommunityDmlSingleInsert.newBuilder()
                    .addColumns(dmlColumn("id", "BIGINT"))
                    .addColumns(dmlColumn("label", "VARCHAR"))
                    .addColumns(dmlColumn("active", "BOOLEAN"))
                    .addColumns(dmlColumn("created_at", "TIMESTAMP"))
                    .addColumns(dmlColumn("note", "VARCHAR"))
                    .setRow(dmlRow(
                            dmlDecimal("7"),
                            dmlString("O'Brien"),
                            dmlBoolean(true),
                            dmlTemporal("2026-07-27T12:34:56"),
                            dmlNull())));
            String singleSql = registry.buildDml(single).getSql();
            assertEquals(
                    "INSERT INTO APP.items (id,label,active,created_at,note)  VALUES "
                            + "('7','O''Brien',TRUE,'2026-07-27 12:34:56',NULL)",
                    singleSql);

            BuildCommunityDmlRequest multi = dmlRequest(CommunityDmlMultiInsert.newBuilder()
                    .addColumns(dmlColumn("id", "BIGINT"))
                    .addColumns(dmlColumn("label", "VARCHAR"))
                    .addRows(dmlRow(dmlDecimal("1"), dmlString("first")))
                    .addRows(dmlRow(dmlDecimal("2"), dmlString("second"))));
            String multiSql = registry.buildDml(multi).getSql();
            assertEquals(
                    "INSERT INTO APP.items (id,label)  VALUES ('1','first'),\n('2','second')",
                    multiSql);

            BuildCommunityDmlRequest update = dmlRequest(CommunityDmlUpdate.newBuilder()
                    .addAssignments(dmlAssignment("label", "VARCHAR", dmlString("next")))
                    .addAssignments(dmlAssignment("active", "BOOLEAN", dmlBoolean(false)))
                    .addPredicates(dmlAssignment("id", "BIGINT", dmlDecimal("7")))
                    .addPredicates(dmlAssignment("active", "BOOLEAN", dmlBoolean(true))));
            String updateSql = registry.buildDml(update).getSql();
            assertEquals(
                    "UPDATE APP.items SET label = 'next',active = FALSE "
                            + "WHERE id = '7' AND active = TRUE",
                    updateSql);

            try (Statement statement = connection.createStatement()) {
                assertEquals(1, statement.executeUpdate(singleSql));
                assertEquals(2, statement.executeUpdate(multiSql));
                assertEquals(1, statement.executeUpdate(updateSql));
                try (var result = statement.executeQuery(
                        "SELECT label, active, created_at, note FROM APP.items WHERE id = 7")) {
                    assertTrue(result.next());
                    assertEquals("next", result.getString(1));
                    assertFalse(result.getBoolean(2));
                    assertEquals("2026-07-27 12:34:56.0", result.getTimestamp(3).toString());
                    assertNull(result.getString(4));
                }
            }

            BuildCommunityDmlRequest binary = dmlRequest(CommunityDmlSingleInsert.newBuilder()
                    .addColumns(dmlColumn("id", "BIGINT"))
                    .addColumns(dmlColumn("payload", "VARBINARY"))
                    .setRow(dmlRow(
                            dmlDecimal("9"),
                            CommunityDmlValue.newBuilder()
                                    .setBinaryValue(ByteString.copyFrom(new byte[] {0, (byte) 0xff}))
                                    .build())));
            assertEquals(
                    "community.dml_value_not_supported",
                    assertFailureCode(
                                    "community.dml_value_not_supported",
                                    () -> registry.buildDml(binary))
                            .code());
        }
    }

    @Test
    void realCommunityMysqlBuildsBoundedTablePreviewSqlWithoutOpeningJdbc()
            throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    hasPlugin(registry, "MYSQL"),
                    "the fixed classpath does not contain the MySQL plugin");
            var descriptor = registry.catalog().getPluginsList().stream()
                    .filter(plugin -> plugin.getDatabaseType().equalsIgnoreCase("MYSQL"))
                    .findFirst()
                    .orElseThrow();
            assertTrue(descriptor.getDqlBuilderAvailable());

            ClassLoader previous = Thread.currentThread().getContextClassLoader();
            var built = registry.buildTablePreviewSql(
                    BuildCommunityTablePreviewSqlRequest.newBuilder()
                            .setDatabaseType("MYSQL")
                            .setDatabaseName("inventory")
                            .setTableName("order")
                            .setRowLimit(200)
                            .build());

            assertEquals(previous, Thread.currentThread().getContextClassLoader());
            assertEquals(200, built.getRowLimit());
            assertTrue(
                    built.getSql().contains("FROM `inventory`.`order`"),
                    built.getSql());
            assertTrue(built.getSql().endsWith("LIMIT 200"), built.getSql());
        }
    }

    @Test
    void realCommunityMysqlRejectsBackslashCrossColumnInjection() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        String payload = "); DROP TABLE audit_log; -- ";
        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    registry.catalog().getPluginsList().stream()
                            .anyMatch(plugin -> plugin.getDatabaseType().equalsIgnoreCase("MYSQL")),
                    "the fixed Community classpath does not contain the MySQL plugin");
            BuildCommunityDmlRequest request = BuildCommunityDmlRequest.newBuilder()
                    .setDatabaseType("MYSQL")
                    .setTarget(CommunityDmlTarget.newBuilder()
                            .setDatabaseName("app")
                            .setTableName("items"))
                    .setSingleInsert(CommunityDmlSingleInsert.newBuilder()
                            .addColumns(dmlColumn("a", "TIMESTAMP"))
                            .addColumns(dmlColumn("b", "TIMESTAMP"))
                            .setRow(dmlRow(dmlString("\\"), dmlString(payload))))
                    .build();

            RuntimeFailure failure = assertFailureCode(
                    "community.dml_value_not_supported", () -> registry.buildDml(request));
            assertFalse(failure.getMessage().contains(payload));
        }
    }

    @Test
    void realCommunityMysqlNormalizesBooleanAliasesAndBits() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    registry.catalog().getPluginsList().stream()
                            .anyMatch(plugin -> plugin.getDatabaseType().equalsIgnoreCase("MYSQL")),
                    "the fixed Community classpath does not contain the MySQL plugin");
            BuildCommunityDmlRequest request = BuildCommunityDmlRequest.newBuilder()
                    .setDatabaseType("MYSQL")
                    .setTarget(CommunityDmlTarget.newBuilder()
                            .setDatabaseName("app")
                            .setTableName("items"))
                    .setSingleInsert(CommunityDmlSingleInsert.newBuilder()
                            .addColumns(dmlColumn("boolean_alias", "BOOLEAN"))
                            .addColumns(dmlColumn("bit_value", "BIT"))
                            .setRow(dmlRow(dmlBoolean(true), dmlBoolean(false))))
                    .build();

            String sql = registry.buildDml(request).getSql();
            assertTrue(sql.contains("('1',b'0')"));
            assertFalse(sql.contains("'true'"));
            assertFalse(sql.contains("'false'"));
        }
    }

    @Test
    void realCommunitySqlServerOwnsIdentifierQuotingAndUsesNumericBits() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
            assumeTrue(
                    registry.catalog().getPluginsList().stream()
                            .anyMatch(plugin ->
                                    plugin.getDatabaseType().equalsIgnoreCase("SQLSERVER")),
                    "the fixed Community classpath does not contain the SQL Server plugin");
            BuildCommunityDmlRequest request = BuildCommunityDmlRequest.newBuilder()
                    .setDatabaseType("SQLSERVER")
                    .setTarget(CommunityDmlTarget.newBuilder()
                            .setDatabaseName("app")
                            .setSchemaName("dbo")
                            .setTableName("items"))
                    .setSingleInsert(CommunityDmlSingleInsert.newBuilder()
                            .addColumns(dmlColumn("select", "BIT"))
                            .setRow(dmlRow(dmlBoolean(true))))
                    .build();

            String sql = registry.buildDml(request).getSql();
            assertTrue(sql.contains("[app].[dbo].[items]"));
            assertTrue(sql.contains("([select])"));
            assertTrue(sql.contains("(1)"));
            assertFalse(sql.contains("[["));

            BuildCommunityDmlRequest update = BuildCommunityDmlRequest.newBuilder()
                    .setDatabaseType("SQLSERVER")
                    .setTarget(request.getTarget())
                    .setUpdate(CommunityDmlUpdate.newBuilder()
                            .addAssignments(dmlAssignment("select", "BIT", dmlBoolean(false)))
                            .addPredicates(dmlAssignment("id", "BIGINT", dmlDecimal("7"))))
                    .build();
            String updateSql = registry.buildDml(update).getSql();
            assertTrue(updateSql.contains("UPDATE [app].[dbo].[items]"));
            assertTrue(updateSql.contains("[select] = 0"));
            assertFalse(updateSql.contains("[["));
        }
    }

    private static void assertMissingDetail(String code, String field) {
        RuntimeFailure failure = assertFailureCode(
                code, () -> CommunityPluginRegistry.requireDetail(null, code, field));
        assertTrue(failure.getMessage().contains("was not found"));
    }

    private static RuntimeFailure assertFailureCode(String code, Executable executable) {
        RuntimeFailure failure = assertThrows(RuntimeFailure.class, executable);
        assertEquals(code, failure.code());
        return failure;
    }

    private static boolean hasPlugin(CommunityPluginRegistry registry, String databaseType)
            throws RuntimeFailure {
        return registry.catalog().getPluginsList().stream()
                .anyMatch(plugin -> plugin.getDatabaseType().equalsIgnoreCase(databaseType));
    }

    private static CommunityPluginRegistry openRegistry(Path directory) throws Exception {
        var paths = CommunityPluginRegistry.validateClasspath(directory.toRealPath().toString());
        URLClassLoader loader = new URLClassLoader(
                paths.stream().map(Path::toUri).map(uri -> {
                    try {
                        return uri.toURL();
                    } catch (java.net.MalformedURLException failure) {
                        throw new IllegalStateException(failure);
                    }
                }).toArray(URL[]::new),
                ClassLoader.getPlatformClassLoader());
        try {
            Method discover = CommunityPluginRegistry.class.getDeclaredMethod(
                    "discover", URLClassLoader.class);
            discover.setAccessible(true);
            @SuppressWarnings("unchecked")
            Map<String, ?> plugins = (Map<String, ?>) discover.invoke(null, loader);
            Constructor<CommunityPluginRegistry> constructor =
                    CommunityPluginRegistry.class.getDeclaredConstructor(
                            String.class,
                            URLClassLoader.class,
                            Map.class,
                            CommunitySqlCompletionBridge.class);
            constructor.setAccessible(true);
            return constructor.newInstance(
                    "37a34be858f2566b6b7fcf6c3f64183c1f560853",
                    loader,
                    plugins,
                    CommunitySqlCompletionBridge.open(loader));
        } catch (Exception | LinkageError failure) {
            loader.close();
            throw failure;
        }
    }

    private static CompleteCommunitySqlRequest completionRequest(
            String databaseName, String sql, int cursorUtf16, long datasourceScope) {
        return CompleteCommunitySqlRequest.newBuilder()
                .setDatabaseType("H2")
                .setDatabaseName(databaseName)
                .setSchemaName("APP")
                .setDatasourceName("Community H2")
                .setSql(sql)
                .setCursorUtf16(cursorUtf16)
                .setKeywordCase("UPPER")
                .setDatasourceScope(datasourceScope)
                .build();
    }

    private static BuildCommunityDmlRequest dmlRequest(
            CommunityDmlSingleInsert.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(dmlTarget())
                .setSingleInsert(statement)
                .build();
    }

    private static BuildCommunityDmlRequest dmlRequest(
            CommunityDmlMultiInsert.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(dmlTarget())
                .setMultiInsert(statement)
                .build();
    }

    private static BuildCommunityDmlRequest dmlRequest(CommunityDmlUpdate.Builder statement) {
        return BuildCommunityDmlRequest.newBuilder()
                .setDatabaseType("H2")
                .setTarget(dmlTarget())
                .setUpdate(statement)
                .build();
    }

    private static CommunityDmlTarget dmlTarget() {
        return CommunityDmlTarget.newBuilder()
                .setSchemaName("APP")
                .setTableName("items")
                .build();
    }

    private static CommunityDmlColumn dmlColumn(String name, String type) {
        return CommunityDmlColumn.newBuilder()
                .setName(name)
                .setDataTypeName(type)
                .build();
    }

    private static CommunityDmlAssignment dmlAssignment(
            String name, String type, CommunityDmlValue value) {
        return CommunityDmlAssignment.newBuilder()
                .setColumn(dmlColumn(name, type))
                .setValue(value)
                .build();
    }

    private static CommunityDmlRow dmlRow(CommunityDmlValue... values) {
        return CommunityDmlRow.newBuilder().addAllValues(List.of(values)).build();
    }

    private static CommunityDmlValue dmlString(String value) {
        return CommunityDmlValue.newBuilder().setStringValue(value).build();
    }

    private static CommunityDmlValue dmlDecimal(String value) {
        return CommunityDmlValue.newBuilder().setDecimalValue(value).build();
    }

    private static CommunityDmlValue dmlBoolean(boolean value) {
        return CommunityDmlValue.newBuilder().setBooleanValue(value).build();
    }

    private static CommunityDmlValue dmlTemporal(String value) {
        return CommunityDmlValue.newBuilder()
                .setTemporalValue(CommunityDmlTemporal.newBuilder()
                        .setKind(CommunityDmlTemporalKind
                                .COMMUNITY_DML_TEMPORAL_KIND_LOCAL_DATETIME)
                        .setIso8601(value))
                .build();
    }

    private static CommunityDmlValue dmlNull() {
        return CommunityDmlValue.newBuilder()
                .setNullValue(CommunityDmlNull.getDefaultInstance())
                .build();
    }

    private static ClassLoader registryLoader(CommunityPluginRegistry registry) throws Exception {
        var field = CommunityPluginRegistry.class.getDeclaredField("loader");
        field.setAccessible(true);
        return (ClassLoader) field.get(registry);
    }

    private static void assertCompletionStateCleared(ClassLoader loader, long datasourceScope)
            throws Exception {
        Class<?> contextType = Class.forName("ai.chat2db.spi.sql.Chat2DBContext", true, loader);
        assertNull(contextType.getMethod("getConnectInfo").invoke(null));
        Class<?> cacheType = Class.forName(
                "ai.chat2db.community.domain.core.cache.MemoryCacheManage", true, loader);
        var cacheField = cacheType.getDeclaredField("CACHE");
        cacheField.setAccessible(true);
        Object cache = cacheField.get(null);
        Class<?> guavaCacheType = Class.forName("com.google.common.cache.Cache", true, loader);
        @SuppressWarnings("unchecked")
        Map<String, ?> entries =
                (Map<String, ?>) guavaCacheType.getMethod("asMap").invoke(cache);
        assertFalse(entries.keySet().stream().anyMatch(
                key -> CommunitySqlCompletionBridge.belongsToDatasourceScope(
                        key, datasourceScope)));
    }

    private static void putCommunityCache(ClassLoader loader, String key, String value)
            throws Exception {
        communityCacheType(loader)
                .getMethod("put", String.class, java.io.Serializable.class)
                .invoke(null, key, value);
    }

    private static Object getCommunityCache(ClassLoader loader, String key) throws Exception {
        return communityCacheType(loader).getMethod("get", String.class).invoke(null, key);
    }

    private static void removeCommunityCache(ClassLoader loader, String key) throws Exception {
        communityCacheType(loader).getMethod("remove", String.class).invoke(null, key);
    }

    private static Class<?> communityCacheType(ClassLoader loader) throws Exception {
        return Class.forName(
                "ai.chat2db.community.domain.core.cache.MemoryCacheManage", true, loader);
    }

    private static Connection h2Connection(ClassLoader loader) throws Exception {
        Driver driver = (Driver) Class.forName("org.h2.Driver", true, loader)
                .getDeclaredConstructor()
                .newInstance();
        Properties properties = new Properties();
        properties.setProperty("user", "sa");
        properties.setProperty("password", "");
        Connection connection = driver.connect(
                "jdbc:h2:mem:community_relations;DB_CLOSE_DELAY=-1", properties);
        assertNotNull(connection);
        return connection;
    }

    private static Path communityClasspathDirectory() {
        String configured = System.getenv(CommunityPluginRegistry.CLASSPATH_ENV);
        if (configured != null && !configured.isBlank()) {
            return Path.of(configured).toAbsolutePath().normalize();
        }
        return Path.of(System.getProperty("basedir"))
                .resolve("../..")
                .resolve("target/community-h2-classpath")
                .toAbsolutePath()
                .normalize();
    }

    private static Path h2DriverJar() throws Exception {
        Path directory = Path.of(System.getProperty("basedir"), "target", "test-drivers");
        try (var paths = Files.list(directory)) {
            return paths.filter(path -> path.getFileName().toString().startsWith("h2-"))
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .findFirst()
                    .orElseThrow()
                    .toRealPath();
        }
    }

    private static void writeResourceJar(Path path, String name, String value) throws Exception {
        try (OutputStream output = Files.newOutputStream(path);
                JarOutputStream jar = new JarOutputStream(output)) {
            jar.putNextEntry(new JarEntry(name));
            jar.write(value.getBytes(StandardCharsets.UTF_8));
            jar.closeEntry();
        }
    }
}

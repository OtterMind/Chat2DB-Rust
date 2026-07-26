package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKey;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKey;
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
    void realCommunityH2ParserValidatesSql() throws Exception {
        Path communityClasspath = communityClasspathDirectory();
        assumeTrue(
                Files.isDirectory(communityClasspath),
                "the fixed Community H2 classpath is built by the extended integration lane");

        try (CommunityPluginRegistry registry = openRegistry(communityClasspath)) {
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
                            String.class, URLClassLoader.class, Map.class);
            constructor.setAccessible(true);
            return constructor.newInstance(
                    "f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7", loader, plugins);
        } catch (Exception | LinkageError failure) {
            loader.close();
            throw failure;
        }
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

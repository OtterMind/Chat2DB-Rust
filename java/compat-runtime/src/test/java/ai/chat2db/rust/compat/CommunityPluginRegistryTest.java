package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKey;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKey;
import java.io.OutputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
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
import org.junit.jupiter.api.Test;
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

package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.OutputStream;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
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

    private static void writeResourceJar(Path path, String name, String value) throws Exception {
        try (OutputStream output = Files.newOutputStream(path);
                JarOutputStream jar = new JarOutputStream(output)) {
            jar.putNextEntry(new JarEntry(name));
            jar.write(value.getBytes(StandardCharsets.UTF_8));
            jar.closeEntry();
        }
    }
}

package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
import com.google.protobuf.ByteString;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.jar.Attributes;
import java.util.jar.JarOutputStream;
import java.util.jar.Manifest;
import java.util.stream.IntStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class DriverRegistryTest {

    @Test
    void derivesStableEngineOwnedDriverIdFromClassAndOrderedDigests() {
        byte[] first = bytes(0, 32);
        byte[] second = bytes(32, 64);

        String driverId = DriverRegistry.deriveDriverId("org.h2.Driver", first);

        assertEquals(
                "sha256:7668f940329b5cbd3854e8692e92bd944405d41361d79e98fea7998bbe47d720",
                driverId);
        assertNotEquals(
                DriverRegistry.deriveDriverId("org.h2.Driver", first, second),
                DriverRegistry.deriveDriverId("org.h2.Driver", second, first));
        assertNotEquals(
                driverId,
                DriverRegistry.deriveDriverId("org.example.OtherDriver", first));
    }

    @Test
    void rejectsManifestClassPathAndDigestMismatchWithoutLeavingSnapshots(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        Path manifestJar = temporaryDirectory.resolve("manifest-driver.jar");
        Manifest manifest = new Manifest();
        manifest.getMainAttributes().put(Attributes.Name.MANIFEST_VERSION, "1.0");
        manifest.getMainAttributes().put(Attributes.Name.CLASS_PATH, "dependency.jar");
        try (OutputStream output = Files.newOutputStream(manifestJar);
                JarOutputStream ignored = new JarOutputStream(output, manifest)) {
            // The manifest itself is enough to reject the artifact before classloading.
        }

        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            RuntimeFailure manifestFailure = assertThrows(
                    RuntimeFailure.class,
                    () -> registry.load(request(manifestJar, sha256(manifestJar))));
            assertEquals("driver.manifest_class_path_unsupported", manifestFailure.code());
            assertDirectoryEmpty(snapshotRoot);

            RuntimeFailure digestFailure = assertThrows(
                    RuntimeFailure.class,
                    () -> registry.load(request(manifestJar, new byte[32])));
            assertEquals("driver.sha256_mismatch", digestFailure.code());
            assertDirectoryEmpty(snapshotRoot);
        }
    }

    private static LoadDriverRequest request(Path jar, byte[] digest) throws Exception {
        return LoadDriverRequest.newBuilder()
                .setDriverClass("example.Driver")
                .addArtifacts(DriverArtifact.newBuilder()
                        .setPath(jar.toRealPath().toString())
                        .setSha256(ByteString.copyFrom(digest)))
                .build();
    }

    private static byte[] sha256(Path path) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (var input = Files.newInputStream(path)) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
            }
        }
        return digest.digest();
    }

    private static void assertDirectoryEmpty(Path directory) throws Exception {
        try (var entries = Files.list(directory)) {
            assertEquals(0, entries.count());
        }
    }

    private static byte[] bytes(int startInclusive, int endExclusive) {
        int[] values = IntStream.range(startInclusive, endExclusive).toArray();
        byte[] bytes = new byte[values.length];
        for (int index = 0; index < values.length; index++) {
            bytes[index] = (byte) values[index];
        }
        return bytes;
    }
}

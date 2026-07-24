package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.sql.Connection;
import java.sql.Driver;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.jar.Attributes;
import java.util.jar.JarFile;
import java.util.jar.Manifest;

/** Isolated registry for explicitly supplied JDBC driver artifacts. */
final class DriverRegistry implements AutoCloseable {

    private final Map<String, LoadedDriver> drivers = new HashMap<>();
    private final Path snapshotRoot;
    private boolean closed;

    DriverRegistry() {
        this(null);
    }

    DriverRegistry(Path snapshotRoot) {
        this.snapshotRoot = snapshotRoot;
    }

    synchronized DriverDescriptor load(LoadDriverRequest request) throws RuntimeFailure {
        ensureOpen();
        ProtocolLimits.requireNonBlankUtf8(
                request.getDriverClass(), ProtocolLimits.MAX_DRIVER_CLASS_BYTES, "driver_class");
        if (request.getArtifactsCount() == 0) {
            throw RuntimeFailure.validation(
                    "driver.artifact_required", "at least one external driver JAR is required");
        }
        if (request.getArtifactsCount() > ProtocolLimits.MAX_DRIVER_ARTIFACTS) {
            throw RuntimeFailure.limit("driver_artifacts", ProtocolLimits.MAX_DRIVER_ARTIFACTS);
        }
        DriverSnapshot snapshot = snapshotArtifacts(request.getArtifactsList(), snapshotRoot);
        List<SnapshotArtifact> artifacts = snapshot.artifacts();
        String driverId = deriveDriverId(
                request.getDriverClass(),
                artifacts.stream().map(SnapshotArtifact::sha256).toArray(byte[][]::new));
        if (drivers.containsKey(driverId)) {
            deleteSnapshotQuietly(snapshot);
            throw RuntimeFailure.conflict(
                    "driver.already_loaded", "the requested JDBC driver is already loaded");
        }
        URLClassLoader loader = null;
        try {
            URL[] urls = artifacts.stream()
                    .map(SnapshotArtifact::path)
                    .map(DriverRegistry::toUrl)
                    .toArray(URL[]::new);
            loader = new URLClassLoader(urls, ClassLoader.getPlatformClassLoader());
            Class<?> rawType = Class.forName(request.getDriverClass(), true, loader);
            if (rawType.getClassLoader() != loader || !Driver.class.isAssignableFrom(rawType)) {
                throw RuntimeFailure.validation(
                        "driver.invalid_class", "driver_class must name a JDBC Driver in the supplied JARs");
            }
            Driver driver = (Driver) rawType.getDeclaredConstructor().newInstance();
            LoadedDriver loaded = new LoadedDriver(
                    driverId,
                    request.getDriverClass(),
                    loader,
                    driver,
                    artifacts.size(),
                    snapshot);
            drivers.put(driverId, loaded);
            loader = null;
            snapshot = null;
            return loaded.descriptor();
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (ReflectiveOperationException | LinkageError failure) {
            throw RuntimeFailure.internal(
                    "driver.load_failed", "the JDBC driver class could not be instantiated", failure);
        } finally {
            if (loader != null) {
                closeQuietly(loader);
            }
            if (snapshot != null) {
                deleteSnapshotQuietly(snapshot);
            }
        }
    }

    synchronized DriverLease acquire(String driverId) throws RuntimeFailure {
        ensureOpen();
        ProtocolLimits.requireNonBlankUtf8(
                driverId, ProtocolLimits.MAX_DRIVER_ID_BYTES, "driver_id");
        LoadedDriver loaded = drivers.get(driverId);
        if (loaded == null) {
            throw RuntimeFailure.validation("driver.not_found", "the requested driver_id is not loaded");
        }
        loaded.references++;
        return new DriverLease(this, loaded);
    }

    synchronized DriverDescriptor unload(String driverId) throws RuntimeFailure {
        ensureOpen();
        ProtocolLimits.requireNonBlankUtf8(
                driverId, ProtocolLimits.MAX_DRIVER_ID_BYTES, "driver_id");
        LoadedDriver loaded = drivers.get(driverId);
        if (loaded == null) {
            throw RuntimeFailure.validation("driver.not_found", "the requested driver_id is not loaded");
        }
        if (loaded.references != 0) {
            throw RuntimeFailure.conflict(
                    "driver.in_use", "the JDBC driver still has open sessions");
        }
        try {
            loaded.loader.close();
            loaded.snapshot.delete();
        } catch (IOException failure) {
            throw RuntimeFailure.internal(
                    "driver.unload_failed", "the JDBC driver snapshot could not be cleaned up", failure);
        }
        drivers.remove(driverId);
        return loaded.descriptor();
    }

    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        closed = true;
        for (LoadedDriver loaded : drivers.values()) {
            closeQuietly(loaded.loader);
            deleteSnapshotQuietly(loaded.snapshot);
        }
        drivers.clear();
    }

    private synchronized void release(LoadedDriver loaded) {
        if (loaded.references > 0) {
            loaded.references--;
        }
    }

    private void ensureOpen() throws RuntimeFailure {
        if (closed) {
            throw RuntimeFailure.conflict("driver.registry_closed", "the driver registry is closed");
        }
    }

    private static DriverSnapshot snapshotArtifacts(
            List<DriverArtifact> requested, Path snapshotRoot)
            throws RuntimeFailure {
        Path snapshotDirectory;
        try {
            snapshotDirectory = snapshotRoot == null
                    ? Files.createTempDirectory("chat2db-jdbc-driver-")
                    : Files.createTempDirectory(snapshotRoot, "chat2db-jdbc-driver-");
        } catch (IOException failure) {
            throw RuntimeFailure.internal(
                    "driver.snapshot_failed", "a private driver snapshot could not be created", failure);
        }
        List<SnapshotArtifact> verified = new ArrayList<>(requested.size());
        Set<Path> uniquePaths = new HashSet<>();
        try {
            for (int index = 0; index < requested.size(); index++) {
                DriverArtifact artifact = requested.get(index);
            ProtocolLimits.requireNonBlankUtf8(
                    artifact.getPath(), ProtocolLimits.MAX_PATH_BYTES, "driver_artifact_path");
            if (artifact.getSha256().size() != 32) {
                throw RuntimeFailure.validation(
                        "driver.invalid_sha256", "driver artifact sha256 must contain exactly 32 bytes");
            }

            Path supplied;
            try {
                supplied = Path.of(artifact.getPath());
            } catch (RuntimeException failure) {
                throw RuntimeFailure.validation(
                        "driver.invalid_artifact_path", "driver artifact path is invalid");
            }
            if (!supplied.isAbsolute()) {
                throw RuntimeFailure.validation(
                        "driver.non_canonical_artifact", "driver artifact path must be absolute and canonical");
            }

                try {
                Path normalized = supplied.normalize();
                Path canonical = supplied.toRealPath(LinkOption.NOFOLLOW_LINKS);
                if (!normalized.equals(canonical)
                        || Files.isSymbolicLink(canonical)
                        || !Files.isRegularFile(canonical, LinkOption.NOFOLLOW_LINKS)
                        || !Files.isReadable(canonical)
                        || !canonical.getFileName().toString().toLowerCase().endsWith(".jar")) {
                    throw RuntimeFailure.validation(
                            "driver.non_canonical_artifact",
                            "driver artifact must be a readable canonical JAR path");
                }
                if (!uniquePaths.add(canonical)) {
                    throw RuntimeFailure.validation(
                            "driver.duplicate_artifact", "driver artifact paths must be unique");
                }
                byte[] expected = artifact.getSha256().toByteArray();
                Path snapshotPath = snapshotDirectory.resolve("artifact-%02d.jar".formatted(index));
                byte[] actual = copyAndHash(canonical, snapshotPath);
                if (!MessageDigest.isEqual(expected, actual)) {
                    throw RuntimeFailure.validation(
                            "driver.sha256_mismatch", "driver artifact digest does not match sha256");
                }
                rejectManifestClassPath(snapshotPath);
                verified.add(new SnapshotArtifact(
                        snapshotPath, Arrays.copyOf(actual, actual.length)));
                } catch (IOException failure) {
                    throw RuntimeFailure.internal(
                            "driver.artifact_unavailable", "driver artifact could not be snapshotted", failure);
                }
            }
            return new DriverSnapshot(snapshotDirectory, List.copyOf(verified));
        } catch (RuntimeFailure failure) {
            deleteSnapshotQuietly(new DriverSnapshot(snapshotDirectory, List.copyOf(verified)));
            throw failure;
        }
    }

    private static byte[] copyAndHash(Path source, Path target) throws IOException {
        MessageDigest digest = newSha256();
        try (InputStream input = Files.newInputStream(source);
                OutputStream output = Files.newOutputStream(target)) {
            byte[] buffer = new byte[64 * 1024];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
                output.write(buffer, 0, count);
            }
        }
        return digest.digest();
    }

    private static void rejectManifestClassPath(Path path) throws IOException, RuntimeFailure {
        try (JarFile jar = new JarFile(path.toFile(), true)) {
            Manifest manifest = jar.getManifest();
            if (manifest != null) {
                String classPath = manifest.getMainAttributes().getValue(Attributes.Name.CLASS_PATH);
                if (classPath != null && !classPath.isBlank()) {
                    throw RuntimeFailure.validation(
                            "driver.manifest_class_path_unsupported",
                            "driver JAR manifests must not declare Class-Path");
                }
            }
        }
    }

    static String deriveDriverId(String driverClass, byte[]... artifactDigests) {
        MessageDigest digest = newSha256();
        digest.update("chat2db-jdbc-driver-v1\0".getBytes(java.nio.charset.StandardCharsets.UTF_8));
        digest.update(driverClass.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        digest.update((byte) 0);
        for (byte[] artifactDigest : artifactDigests) {
            digest.update(artifactDigest);
        }
        return "sha256:" + HexFormat.of().formatHex(digest.digest());
    }

    private static MessageDigest newSha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException impossible) {
            throw new IllegalStateException("SHA-256 is required by the Java runtime", impossible);
        }
    }

    private static URL toUrl(Path path) {
        try {
            return path.toUri().toURL();
        } catch (IOException impossible) {
            throw new IllegalArgumentException("canonical artifact path could not become a URL", impossible);
        }
    }

    private static void closeQuietly(URLClassLoader loader) {
        try {
            loader.close();
        } catch (IOException ignored) {
            // Best effort during failed construction or process shutdown.
        }
    }

    private static void deleteSnapshotQuietly(DriverSnapshot snapshot) {
        try {
            snapshot.delete();
        } catch (IOException ignored) {
            // Process-exit cleanup remains the final fallback.
        }
    }

    record DriverDescriptor(String driverId, String driverClass, int artifactCount) {
    }

    static final class DriverLease implements AutoCloseable {
        private final DriverRegistry registry;
        private final LoadedDriver loaded;
        private final AtomicBoolean closed = new AtomicBoolean();

        private DriverLease(DriverRegistry registry, LoadedDriver loaded) {
            this.registry = registry;
            this.loaded = loaded;
        }

        Connection connect(String jdbcUrl, Properties properties) throws SQLException {
            if (closed.get()) {
                throw new SQLException("driver lease is closed");
            }
            Thread thread = Thread.currentThread();
            ClassLoader previous = thread.getContextClassLoader();
            try {
                thread.setContextClassLoader(loaded.loader);
                return loaded.driver.connect(jdbcUrl, properties);
            } finally {
                thread.setContextClassLoader(previous);
            }
        }

        @Override
        public void close() {
            if (closed.compareAndSet(false, true)) {
                registry.release(loaded);
            }
        }
    }

    private static final class LoadedDriver {
        private final String id;
        private final String className;
        private final URLClassLoader loader;
        private final Driver driver;
        private final int artifactCount;
        private final DriverSnapshot snapshot;
        private int references;

        private LoadedDriver(
                String id,
                String className,
                URLClassLoader loader,
                Driver driver,
                int artifactCount,
                DriverSnapshot snapshot) {
            this.id = id;
            this.className = className;
            this.loader = loader;
            this.driver = driver;
            this.artifactCount = artifactCount;
            this.snapshot = snapshot;
        }

        private DriverDescriptor descriptor() {
            return new DriverDescriptor(id, className, artifactCount);
        }
    }

    private record SnapshotArtifact(Path path, byte[] sha256) {
        private SnapshotArtifact {
            sha256 = Arrays.copyOf(sha256, sha256.length);
        }

        @Override
        public byte[] sha256() {
            return Arrays.copyOf(sha256, sha256.length);
        }
    }

    private record DriverSnapshot(Path directory, List<SnapshotArtifact> artifacts) {
        private void delete() throws IOException {
            IOException failure = null;
            try (var paths = Files.list(directory)) {
                for (Path path : paths.toList()) {
                    try {
                        Files.deleteIfExists(path);
                    } catch (IOException deleteFailure) {
                        failure = append(failure, deleteFailure);
                    }
                }
            } catch (IOException listFailure) {
                failure = append(failure, listFailure);
            }
            try {
                Files.deleteIfExists(directory);
            } catch (IOException deleteFailure) {
                failure = append(failure, deleteFailure);
            }
            if (failure != null) {
                throw failure;
            }
        }

        private static IOException append(IOException current, IOException additional) {
            if (current == null) {
                return additional;
            }
            current.addSuppressed(additional);
            return current;
        }
    }
}

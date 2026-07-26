import java.io.BufferedOutputStream;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Enumeration;
import java.util.HashMap;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.jar.Attributes;
import java.util.jar.Manifest;
import java.util.zip.CRC32;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

/** Deterministically canonicalizes the fixed Community compatibility classpath. */
public final class CommunityClasspathSanitizer {
    private static final String MANIFEST_PATH = "META-INF/MANIFEST.MF";
    private static final String COMMUNITY_MAVEN_ROOT = "META-INF/maven/ai.chat2db/";
    private static final String COMMUNITY_GROUP_ID = "ai.chat2db";
    private static final String COMMUNITY_VERSION = "5.3.0";
    private static final long MAX_ENTRY_BYTES = 64L * 1024 * 1024;
    private static final long MAX_ARCHIVE_BYTES = 512L * 1024 * 1024;
    private static final long MAX_MANIFEST_BYTES = 8L * 1024 * 1024;
    private static final byte[] CLASS_PATH_HEADER =
            "Class-Path".getBytes(StandardCharsets.US_ASCII);
    private static final Map<String, SourceArtifact> EXPECTED_SOURCES = Map.of(
            "jaxb-runtime-2.3.1.jar",
            new SourceArtifact(
                    1_093_432L,
                    "45fecfa5c8217ce1f3652ab95179790ec8cc0dec0384bca51cbeb94a293d9f2f"));
    private static final Map<String, CommunityArtifact> COMMUNITY_ARTIFACTS = Map.of(
            "chat2db-community-domain-api-5.3.0.jar",
            new CommunityArtifact("chat2db-community-domain-api"),
            "chat2db-community-h2-5.3.0.jar",
            new CommunityArtifact("chat2db-community-h2"),
            "chat2db-community-mysql-5.3.0.jar",
            new CommunityArtifact("chat2db-community-mysql"),
            "chat2db-community-spi-5.3.0.jar",
            new CommunityArtifact("chat2db-community-spi"),
            "chat2db-community-tools-5.3.0.jar",
            new CommunityArtifact("chat2db-community-tools"));

    private CommunityClasspathSanitizer() {}

    public static void main(String[] arguments) {
        try {
            requireJava17();
            if (arguments.length == 3 && arguments[0].equals("sanitize")) {
                Path directory = requireDirectory(arguments[1]);
                long timestamp = parseTimestamp(arguments[2]);
                sanitize(directory, timestamp);
                verify(directory);
                return;
            }
            if (arguments.length == 2 && arguments[0].equals("verify")) {
                verify(requireDirectory(arguments[1]));
                return;
            }
            if (arguments.length == 1 && arguments[0].equals("self-test")) {
                selfTest();
                return;
            }
            if (arguments.length == 2 && arguments[0].equals("sha256")) {
                Path file = Path.of(arguments[1]);
                if (Files.isSymbolicLink(file)
                        || !Files.isRegularFile(file, LinkOption.NOFOLLOW_LINKS)) {
                    throw new IOException(
                            "hash input must be a non-symbolic regular file: " + file);
                }
                System.out.println(sha256(file));
                return;
            }
            throw new IllegalArgumentException(
                    "usage: CommunityClasspathSanitizer.java "
                            + "<sanitize <directory> <epoch-seconds>|verify <directory>|self-test"
                            + "|sha256 <file>>");
        } catch (Exception failure) {
            System.err.println("Community classpath sanitization failed: " + failure.getMessage());
            System.exit(1);
        }
    }

    private static void requireJava17() {
        int feature = Runtime.version().feature();
        if (feature != 17) {
            throw new IllegalStateException("JDK 17 is required; found Java " + feature);
        }
    }

    private static Path requireDirectory(String value) throws IOException {
        Path supplied = Path.of(value);
        if (Files.isSymbolicLink(supplied)
                || !Files.isDirectory(supplied, LinkOption.NOFOLLOW_LINKS)) {
            throw new IOException("classpath must be a non-symbolic directory: " + supplied);
        }
        return supplied.toRealPath(LinkOption.NOFOLLOW_LINKS);
    }

    private static long parseTimestamp(String value) {
        long seconds;
        try {
            seconds = Long.parseLong(value);
        } catch (NumberFormatException failure) {
            throw new IllegalArgumentException("archive timestamp must be epoch seconds", failure);
        }
        int year = LocalDateTime.ofInstant(Instant.ofEpochSecond(seconds), ZoneOffset.UTC)
                .getYear();
        if (year < 1980 || year > 2107) {
            throw new IllegalArgumentException("archive timestamp is outside the ZIP date range");
        }
        return seconds;
    }

    private static void sanitize(Path directory, long timestamp) throws IOException {
        LocalDateTime archiveTime =
                LocalDateTime.ofInstant(Instant.ofEpochSecond(timestamp), ZoneOffset.UTC);
        Set<String> missingCommunityArtifacts = new HashSet<>(COMMUNITY_ARTIFACTS.keySet());
        int sanitizedManifests = 0;
        int canonicalizedCommunityArtifacts = 0;
        for (Path artifact : artifacts(directory)) {
            String filename = artifact.getFileName().toString();
            SourceArtifact expected = EXPECTED_SOURCES.get(filename);
            CommunityArtifact communityArtifact = COMMUNITY_ARTIFACTS.get(filename);
            if (expected != null) {
                requireExpectedSource(artifact, expected);
                if (!hasNonEmptyManifestClassPath(artifact)) {
                    throw new IOException(
                            "expected source JAR no longer declares Class-Path: "
                                    + artifact.getFileName());
                }
                rebuild(artifact, archiveTime, RebuildPlan.manifestClassPath());
                sanitizedManifests++;
                System.out.println("Sanitized manifest Class-Path: " + artifact.getFileName());
            } else if (communityArtifact != null) {
                if (hasNonEmptyManifestClassPath(artifact)) {
                    throw new IOException(
                            "Community project JAR unexpectedly declares Class-Path: "
                                    + artifact.getFileName());
                }
                rebuild(artifact, archiveTime, RebuildPlan.community(communityArtifact));
                missingCommunityArtifacts.remove(filename);
                canonicalizedCommunityArtifacts++;
                System.out.println("Canonicalized Community project JAR: " + artifact.getFileName());
            } else if (hasNonEmptyManifestClassPath(artifact)) {
                throw new IOException(
                        "unexpected JAR declares Class-Path and has no pinned transformation: "
                                + artifact.getFileName());
            }
        }
        if (sanitizedManifests != EXPECTED_SOURCES.size()) {
            throw new IOException(
                    "expected to sanitize "
                            + EXPECTED_SOURCES.size()
                            + " pinned JAR(s); found "
                            + sanitizedManifests);
        }
        if (!missingCommunityArtifacts.isEmpty()) {
            throw new IOException(
                    "missing Community project JAR(s): "
                            + String.join(", ", missingCommunityArtifacts.stream().sorted().toList()));
        }
        System.out.println(
                "Sanitized " + sanitizedManifests + " manifest Class-Path JAR(s) and canonicalized "
                        + canonicalizedCommunityArtifacts + " Community project JAR(s)");
    }

    private static void verify(Path directory) throws IOException {
        int count = 0;
        Set<String> missingCommunityArtifacts = new HashSet<>(COMMUNITY_ARTIFACTS.keySet());
        for (Path artifact : artifacts(directory)) {
            count++;
            if (hasNonEmptyManifestClassPath(artifact)) {
                throw new IOException(
                        "manifest still declares a non-empty Class-Path: "
                                + artifact.getFileName());
            }
            String filename = artifact.getFileName().toString();
            CommunityArtifact communityArtifact = COMMUNITY_ARTIFACTS.get(filename);
            if (communityArtifact != null) {
                verifyCommunityDescriptor(artifact, communityArtifact);
                missingCommunityArtifacts.remove(filename);
            }
        }
        if (!missingCommunityArtifacts.isEmpty()) {
            throw new IOException(
                    "missing Community project JAR(s): "
                            + String.join(", ", missingCommunityArtifacts.stream().sorted().toList()));
        }
        System.out.println(
                "Verified " + count
                        + " Community classpath JAR manifest(s) and canonical project descriptor(s)");
    }

    private static List<Path> artifacts(Path directory) throws IOException {
        List<Path> entries;
        try (var paths = Files.list(directory)) {
            entries = paths.sorted(Comparator.comparing(path -> path.getFileName().toString()))
                    .toList();
        }
        if (entries.isEmpty()) {
            throw new IOException("Community classpath is empty: " + directory);
        }
        for (Path entry : entries) {
            String filename = entry.getFileName().toString();
            if (Files.isSymbolicLink(entry)
                    || !Files.isRegularFile(entry, LinkOption.NOFOLLOW_LINKS)
                    || !filename.endsWith(".jar")) {
                throw new IOException("unexpected Community classpath entry: " + entry);
            }
        }
        return entries;
    }

    private static boolean hasNonEmptyManifestClassPath(Path artifact) throws IOException {
        try (ZipFile jar = new ZipFile(artifact.toFile(), StandardCharsets.UTF_8)) {
            ZipEntry entry = jar.getEntry(MANIFEST_PATH);
            if (entry == null) {
                return false;
            }
            if (entry.isDirectory()
                    || entry.getSize() < 0
                    || entry.getSize() > MAX_MANIFEST_BYTES) {
                throw new IOException("invalid or oversized JAR manifest: " + artifact);
            }
            Manifest manifest;
            try (InputStream input = jar.getInputStream(entry)) {
                manifest = new Manifest(input);
            }
            String classPath = manifest.getMainAttributes().getValue(Attributes.Name.CLASS_PATH);
            return classPath != null && !classPath.isBlank();
        }
    }

    private static void requireExpectedSource(Path artifact, SourceArtifact expected)
            throws IOException {
        long size = Files.size(artifact);
        if (size != expected.size()) {
            throw new IOException(
                    "source JAR length drifted for " + artifact.getFileName() + ": " + size);
        }
        String digest = sha256(artifact);
        if (!digest.equals(expected.sha256())) {
            throw new IOException(
                    "source JAR SHA-256 drifted for " + artifact.getFileName() + ": " + digest);
        }
    }

    private static void rebuild(
            Path artifact, LocalDateTime archiveTime, RebuildPlan plan) throws IOException {
        Path temporary = Files.createTempFile(
                artifact.getParent(), "." + artifact.getFileName() + ".sanitize-", ".tmp");
        try {
            try (ZipFile source = new ZipFile(artifact.toFile(), StandardCharsets.UTF_8)) {
                List<? extends ZipEntry> entries = orderedEntries(source);
                rejectSignedArchive(entries, artifact);
                Map<String, byte[]> replacements = new HashMap<>();
                if (plan.stripManifestClassPath()) {
                    ZipEntry manifestEntry = source.getEntry(MANIFEST_PATH);
                    if (manifestEntry == null || manifestEntry.isDirectory()) {
                        throw new IOException(
                                "Class-Path was reported without a regular manifest: " + artifact);
                    }
                    if (manifestEntry.getSize() < 0
                            || manifestEntry.getSize() > MAX_MANIFEST_BYTES) {
                        throw new IOException("invalid or oversized JAR manifest: " + artifact);
                    }
                    byte[] sanitizedManifest;
                    try (InputStream input = source.getInputStream(manifestEntry)) {
                        sanitizedManifest = stripMainClassPath(input.readAllBytes());
                    }
                    assertManifestSanitized(sanitizedManifest, artifact);
                    replacements.put(MANIFEST_PATH, sanitizedManifest);
                }
                if (plan.communityArtifact() != null) {
                    replacements.putAll(communityDescriptorReplacements(
                            source, entries, artifact, plan.communityArtifact(), false));
                }

                try (OutputStream file = Files.newOutputStream(temporary);
                        ZipOutputStream target = new ZipOutputStream(
                                new BufferedOutputStream(file), StandardCharsets.UTF_8)) {
                    for (ZipEntry sourceEntry : entries) {
                        byte[] contents;
                        byte[] replacement = replacements.get(sourceEntry.getName());
                        if (replacement != null) {
                            contents = replacement;
                        } else {
                            try (InputStream input = source.getInputStream(sourceEntry)) {
                                contents = input.readAllBytes();
                            }
                        }
                        writeStoredEntry(target, sourceEntry.getName(), contents, archiveTime);
                    }
                }
                verifyRebuiltArchive(source, entries, temporary, replacements, archiveTime);
            }
            try {
                Files.move(
                        temporary,
                        artifact,
                        StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException unsupported) {
                Files.move(temporary, artifact, StandardCopyOption.REPLACE_EXISTING);
            }
        } finally {
            Files.deleteIfExists(temporary);
        }
    }

    private static Map<String, byte[]> communityDescriptorReplacements(
            ZipFile source,
            List<? extends ZipEntry> entries,
            Path artifact,
            CommunityArtifact communityArtifact,
            boolean requireCanonical)
            throws IOException {
        String descriptorDirectory = communityArtifact.descriptorDirectory();
        String pomPath = descriptorDirectory + "pom.xml";
        String propertiesPath = descriptorDirectory + "pom.properties";
        for (ZipEntry entry : entries) {
            String name = entry.getName();
            if (!entry.isDirectory()
                    && name.startsWith(COMMUNITY_MAVEN_ROOT)
                    && !name.equals(pomPath)
                    && !name.equals(propertiesPath)) {
                throw new IOException(
                        "Community project JAR contains an unexpected Maven descriptor: "
                                + artifact.getFileName() + "!" + name);
            }
        }
        ZipEntry pomEntry = requireRegularEntry(source, pomPath, artifact);
        byte[] pom;
        try (InputStream input = source.getInputStream(pomEntry)) {
            pom = input.readAllBytes();
        }
        byte[] canonicalPom = canonicalTextDescriptor(pom, artifact, pomPath);
        if (requireCanonical && !Arrays.equals(pom, canonicalPom)) {
            throw new IOException(
                    "Community Maven pom.xml is not LF-canonical: "
                            + artifact.getFileName() + "!" + pomPath);
        }
        ZipEntry propertiesEntry = requireRegularEntry(source, propertiesPath, artifact);
        byte[] properties;
        try (InputStream input = source.getInputStream(propertiesEntry)) {
            properties = input.readAllBytes();
        }
        byte[] canonical = canonicalCommunityProperties(communityArtifact, properties, artifact);
        if (requireCanonical && !Arrays.equals(properties, canonical)) {
            throw new IOException(
                    "Community Maven descriptor is not LF-canonical: "
                            + artifact.getFileName() + "!" + propertiesPath);
        }
        return Map.of(pomPath, canonicalPom, propertiesPath, canonical);
    }

    private static ZipEntry requireRegularEntry(ZipFile source, String name, Path artifact)
            throws IOException {
        ZipEntry entry = source.getEntry(name);
        if (entry == null || entry.isDirectory()) {
            throw new IOException(
                    "Community project JAR is missing Maven descriptor: "
                            + artifact.getFileName() + "!" + name);
        }
        return entry;
    }

    private static byte[] canonicalCommunityProperties(
            CommunityArtifact communityArtifact, byte[] actual, Path artifact) throws IOException {
        byte[] canonical = communityArtifact.properties("\n");
        byte[] windows = communityArtifact.properties("\r\n");
        if (!Arrays.equals(actual, canonical) && !Arrays.equals(actual, windows)) {
            throw new IOException(
                    "invalid Community Maven descriptor contents: "
                            + artifact.getFileName() + "!" + communityArtifact.propertiesPath());
        }
        return canonical;
    }

    private static byte[] canonicalTextDescriptor(
            byte[] actual, Path artifact, String descriptorPath) throws IOException {
        ByteArrayOutputStream canonical = new ByteArrayOutputStream(actual.length);
        boolean sawLf = false;
        boolean sawCrlf = false;
        for (int index = 0; index < actual.length; index++) {
            int value = actual[index] & 0xff;
            if (value == '\r') {
                if (index + 1 >= actual.length || actual[index + 1] != '\n') {
                    throw new IOException(
                            "invalid Community Maven pom.xml line endings: "
                                    + artifact.getFileName() + "!" + descriptorPath);
                }
                sawCrlf = true;
                canonical.write('\n');
                index++;
            } else {
                if (value == '\n') {
                    sawLf = true;
                }
                canonical.write(value);
            }
        }
        if (sawLf && sawCrlf) {
            throw new IOException(
                    "invalid Community Maven pom.xml line endings: "
                            + artifact.getFileName() + "!" + descriptorPath);
        }
        return canonical.toByteArray();
    }

    private static void verifyCommunityDescriptor(
            Path artifact, CommunityArtifact communityArtifact) throws IOException {
        try (ZipFile source = new ZipFile(artifact.toFile(), StandardCharsets.UTF_8)) {
            List<? extends ZipEntry> entries = orderedEntries(source);
            communityDescriptorReplacements(source, entries, artifact, communityArtifact, true);
        }
    }

    private static List<? extends ZipEntry> orderedEntries(ZipFile source) throws IOException {
        List<ZipEntry> entries = new ArrayList<>();
        Set<String> names = new HashSet<>();
        long totalBytes = 0;
        Enumeration<? extends ZipEntry> enumeration = source.entries();
        while (enumeration.hasMoreElements()) {
            ZipEntry entry = enumeration.nextElement();
            validateEntry(entry);
            if (!names.add(entry.getName())) {
                throw new IOException("JAR contains a duplicate entry: " + entry.getName());
            }
            if (totalBytes > MAX_ARCHIVE_BYTES - entry.getSize()) {
                throw new IOException("JAR uncompressed contents exceed the safety limit");
            }
            totalBytes += entry.getSize();
            entries.add(entry);
        }
        entries.sort(Comparator
                .comparing((ZipEntry entry) -> !entry.getName().equals(MANIFEST_PATH))
                .thenComparing(ZipEntry::getName));
        return entries;
    }

    private static void validateEntry(ZipEntry entry) throws IOException {
        String name = entry.getName();
        if (name.isEmpty()
                || name.indexOf('\0') >= 0
                || name.indexOf('\\') >= 0
                || name.startsWith("/")
                || name.matches("^[A-Za-z]:.*")
                || entry.getSize() < 0
                || entry.getSize() > MAX_ENTRY_BYTES) {
            throw new IOException("JAR contains an unsafe or oversized entry: " + name);
        }
        for (String segment : name.split("/", -1)) {
            if (segment.equals(".") || segment.equals("..")) {
                throw new IOException("JAR contains a traversal entry: " + name);
            }
        }
    }

    private static void rejectSignedArchive(List<? extends ZipEntry> entries, Path artifact)
            throws IOException {
        for (ZipEntry entry : entries) {
            String name = entry.getName().toUpperCase(Locale.ROOT);
            if (!name.startsWith("META-INF/")) {
                continue;
            }
            String leaf = name.substring("META-INF/".length());
            if (!leaf.contains("/")
                    && (leaf.startsWith("SIG-")
                            || leaf.endsWith(".SF")
                            || leaf.endsWith(".RSA")
                            || leaf.endsWith(".DSA")
                            || leaf.endsWith(".EC"))) {
                throw new IOException(
                        "cannot sanitize a signed JAR without invalidating it: " + artifact);
            }
        }
    }

    private static void writeStoredEntry(
            ZipOutputStream target, String name, byte[] contents, LocalDateTime archiveTime)
            throws IOException {
        CRC32 crc = new CRC32();
        crc.update(contents);
        ZipEntry entry = new ZipEntry(name);
        entry.setMethod(ZipEntry.STORED);
        entry.setSize(contents.length);
        entry.setCompressedSize(contents.length);
        entry.setCrc(crc.getValue());
        entry.setTimeLocal(archiveTime);
        target.putNextEntry(entry);
        target.write(contents);
        target.closeEntry();
    }

    private static byte[] stripMainClassPath(byte[] manifest) throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream(manifest.length);
        boolean inMainSection = true;
        boolean removing = false;
        boolean removed = false;
        int cursor = 0;
        while (cursor < manifest.length) {
            int start = cursor;
            while (cursor < manifest.length
                    && manifest[cursor] != '\r'
                    && manifest[cursor] != '\n') {
                cursor++;
            }
            int contentEnd = cursor;
            if (cursor < manifest.length && manifest[cursor] == '\r') {
                cursor++;
                if (cursor < manifest.length && manifest[cursor] == '\n') {
                    cursor++;
                }
            } else if (cursor < manifest.length) {
                cursor++;
            }
            int end = cursor;

            if (inMainSection && contentEnd == start) {
                inMainSection = false;
                removing = false;
                output.write(manifest, start, end - start);
                continue;
            }
            if (inMainSection && contentEnd > start && manifest[start] == ' ') {
                if (!removing) {
                    output.write(manifest, start, end - start);
                }
                continue;
            }
            if (inMainSection) {
                removing = isClassPathHeader(manifest, start, contentEnd);
                if (removing) {
                    removed = true;
                    continue;
                }
            }
            output.write(manifest, start, end - start);
        }
        if (!removed) {
            throw new IOException(
                    "manifest parser found Class-Path but its header could not be removed");
        }
        return output.toByteArray();
    }

    private static boolean isClassPathHeader(byte[] manifest, int start, int end) {
        if (end - start < CLASS_PATH_HEADER.length + 1
                || manifest[start + CLASS_PATH_HEADER.length] != ':') {
            return false;
        }
        for (int index = 0; index < CLASS_PATH_HEADER.length; index++) {
            int actual = manifest[start + index] & 0xff;
            int expected = CLASS_PATH_HEADER[index] & 0xff;
            if (actual >= 'A' && actual <= 'Z') {
                actual += 'a' - 'A';
            }
            if (expected >= 'A' && expected <= 'Z') {
                expected += 'a' - 'A';
            }
            if (actual != expected) {
                return false;
            }
        }
        return true;
    }

    private static void assertManifestSanitized(byte[] contents, Path artifact) throws IOException {
        Manifest manifest = new Manifest(new ByteArrayInputStream(contents));
        String classPath = manifest.getMainAttributes().getValue(Attributes.Name.CLASS_PATH);
        if (classPath != null) {
            throw new IOException("manifest Class-Path removal failed: " + artifact);
        }
    }

    private static void verifyRebuiltArchive(
            ZipFile source,
            List<? extends ZipEntry> orderedSourceEntries,
            Path rebuilt,
            Map<String, byte[]> replacements,
            LocalDateTime archiveTime)
            throws IOException {
        try (ZipFile target = new ZipFile(rebuilt.toFile(), StandardCharsets.UTF_8)) {
            if (target.getComment() != null) {
                throw new IOException("rebuilt JAR unexpectedly has an archive comment");
            }
            Enumeration<? extends ZipEntry> enumeration = target.entries();
            for (ZipEntry sourceEntry : orderedSourceEntries) {
                if (!enumeration.hasMoreElements()) {
                    throw new IOException("rebuilt JAR is missing entries");
                }
                ZipEntry targetEntry = enumeration.nextElement();
                if (!targetEntry.getName().equals(sourceEntry.getName())) {
                    throw new IOException("rebuilt JAR entry order is not deterministic");
                }
                if (targetEntry.getMethod() != ZipEntry.STORED
                        || targetEntry.getExtra() != null
                        || targetEntry.getComment() != null
                        || !targetEntry.getTimeLocal().equals(archiveTime)) {
                    throw new IOException(
                            "rebuilt JAR entry metadata is not canonical: "
                                    + targetEntry.getName());
                }
                byte[] expected;
                byte[] replacement = replacements.get(sourceEntry.getName());
                if (replacement != null) {
                    expected = replacement;
                } else {
                    try (InputStream input = source.getInputStream(sourceEntry)) {
                        expected = input.readAllBytes();
                    }
                }
                byte[] actual;
                try (InputStream input = target.getInputStream(targetEntry)) {
                    actual = input.readAllBytes();
                }
                if (!Arrays.equals(expected, actual)) {
                    throw new IOException(
                            "rebuilt JAR changed entry contents: " + targetEntry.getName());
                }
            }
            if (enumeration.hasMoreElements()) {
                throw new IOException("rebuilt JAR has unexpected entries");
            }
        }
    }

    private static String sha256(Path artifact) throws IOException {
        MessageDigest digest;
        try {
            digest = MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException impossible) {
            throw new IllegalStateException("JDK does not provide SHA-256", impossible);
        }
        try (InputStream input = Files.newInputStream(artifact)) {
            byte[] buffer = new byte[64 * 1024];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
            }
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static void selfTest() throws IOException {
        LocalDateTime archiveTime = LocalDateTime.of(2000, 1, 2, 3, 4, 6);
        Path directory = Files.createTempDirectory("chat2db-community-sanitizer-test-");
        try {
            Path first = directory.resolve("first.jar");
            Path second = directory.resolve("second.jar");
            writeFixture(first, null, false, archiveTime);
            Files.copy(first, second);
            rebuild(first, archiveTime, RebuildPlan.manifestClassPath());
            rebuild(second, archiveTime, RebuildPlan.manifestClassPath());
            if (!Arrays.equals(Files.readAllBytes(first), Files.readAllBytes(second))) {
                throw new IOException("identical sanitizer inputs produced different bytes");
            }
            verifyFixture(first, archiveTime);
            expectSourceRejected(
                    first,
                    new SourceArtifact(Files.size(first) + 1, sha256(first)),
                    "length drifted");
            expectSourceRejected(
                    first,
                    new SourceArtifact(Files.size(first), "0".repeat(64)),
                    "SHA-256 drifted");

            Path signed = directory.resolve("signed.jar");
            writeFixture(signed, "TEST.SF", false, archiveTime);
            expectRejected(
                    signed, archiveTime, RebuildPlan.manifestClassPath(), "signed JAR");

            Path sigPrefix = directory.resolve("sig-prefix.jar");
            writeFixture(sigPrefix, "SIG-TEST", false, archiveTime);
            expectRejected(
                    sigPrefix, archiveTime, RebuildPlan.manifestClassPath(), "signed JAR");

            Path traversal = directory.resolve("traversal.jar");
            writeFixture(traversal, null, true, archiveTime);
            expectRejected(
                    traversal,
                    archiveTime,
                    RebuildPlan.manifestClassPath(),
                    "traversal entry");

            CommunityArtifact communityArtifact = new CommunityArtifact("fixture-artifact");
            Path lfDescriptor = directory.resolve("descriptor-lf.jar");
            Path crlfDescriptor = directory.resolve("descriptor-crlf.jar");
            writeCommunityFixture(
                    lfDescriptor, communityArtifact, communityArtifact.properties("\n"), archiveTime);
            writeCommunityFixture(
                    crlfDescriptor,
                    communityArtifact,
                    communityArtifact.properties("\r\n"),
                    "<project/>\r\n".getBytes(StandardCharsets.UTF_8),
                    true,
                    null,
                    archiveTime);
            RebuildPlan communityPlan = RebuildPlan.community(communityArtifact);
            rebuild(lfDescriptor, archiveTime, communityPlan);
            rebuild(crlfDescriptor, archiveTime, communityPlan);
            if (!Arrays.equals(
                    Files.readAllBytes(lfDescriptor), Files.readAllBytes(crlfDescriptor))) {
                throw new IOException("LF and CRLF descriptors produced different bytes");
            }
            byte[] canonicalBytes = Files.readAllBytes(lfDescriptor);
            rebuild(lfDescriptor, archiveTime, communityPlan);
            if (!Arrays.equals(canonicalBytes, Files.readAllBytes(lfDescriptor))) {
                throw new IOException("Community project JAR canonicalization is not idempotent");
            }
            verifyCommunityDescriptor(lfDescriptor, communityArtifact);

            Path mixedNewlines = directory.resolve("descriptor-mixed-newlines.jar");
            byte[] invalidNewlines = ("artifactId=fixture-artifact\r\n"
                            + "groupId=ai.chat2db\n"
                            + "version=5.3.0\r\n")
                    .getBytes(StandardCharsets.UTF_8);
            writeCommunityFixture(
                    mixedNewlines, communityArtifact, invalidNewlines, archiveTime);
            expectRejected(
                    mixedNewlines,
                    archiveTime,
                    communityPlan,
                    "invalid Community Maven descriptor contents");

            Path wrongCoordinates = directory.resolve("descriptor-wrong-coordinates.jar");
            byte[] invalidCoordinates = ("artifactId=wrong-artifact\n"
                            + "groupId=ai.chat2db\n"
                            + "version=5.3.0\n")
                    .getBytes(StandardCharsets.UTF_8);
            writeCommunityFixture(
                    wrongCoordinates, communityArtifact, invalidCoordinates, archiveTime);
            expectRejected(
                    wrongCoordinates,
                    archiveTime,
                    communityPlan,
                    "invalid Community Maven descriptor contents");

            Path mixedPom = directory.resolve("descriptor-mixed-pom.jar");
            writeCommunityFixture(
                    mixedPom,
                    communityArtifact,
                    communityArtifact.properties("\n"),
                    "<project>\r\n<name>fixture</name>\n</project>\r\n"
                            .getBytes(StandardCharsets.UTF_8),
                    true,
                    null,
                    archiveTime);
            expectRejected(
                    mixedPom,
                    archiveTime,
                    communityPlan,
                    "invalid Community Maven pom.xml line endings");

            Path bareCarriageReturnPom = directory.resolve("descriptor-bare-cr-pom.jar");
            writeCommunityFixture(
                    bareCarriageReturnPom,
                    communityArtifact,
                    communityArtifact.properties("\n"),
                    "<project>\r</project>\n".getBytes(StandardCharsets.UTF_8),
                    true,
                    null,
                    archiveTime);
            expectRejected(
                    bareCarriageReturnPom,
                    archiveTime,
                    communityPlan,
                    "invalid Community Maven pom.xml line endings");

            Path missingDescriptor = directory.resolve("descriptor-missing.jar");
            writeCommunityFixture(
                    missingDescriptor,
                    communityArtifact,
                    communityArtifact.properties("\n"),
                    "<project/>\n".getBytes(StandardCharsets.UTF_8),
                    false,
                    null,
                    archiveTime);
            expectRejected(
                    missingDescriptor,
                    archiveTime,
                    communityPlan,
                    "missing Maven descriptor");

            Path extraDescriptor = directory.resolve("descriptor-extra.jar");
            writeCommunityFixture(
                    extraDescriptor,
                    communityArtifact,
                    communityArtifact.properties("\n"),
                    "<project/>\n".getBytes(StandardCharsets.UTF_8),
                    true,
                    COMMUNITY_MAVEN_ROOT + "unexpected/pom.xml",
                    archiveTime);
            expectRejected(
                    extraDescriptor,
                    archiveTime,
                    communityPlan,
                    "unexpected Maven descriptor");
        } finally {
            try (var entries = Files.list(directory)) {
                for (Path entry : entries.toList()) {
                    Files.deleteIfExists(entry);
                }
            }
            Files.deleteIfExists(directory);
        }
        System.out.println("Community classpath sanitizer self-test passed");
    }

    private static void writeCommunityFixture(
            Path path,
            CommunityArtifact communityArtifact,
            byte[] properties,
            LocalDateTime archiveTime)
            throws IOException {
        writeCommunityFixture(
                path,
                communityArtifact,
                properties,
                "<project/>\n".getBytes(StandardCharsets.UTF_8),
                true,
                null,
                archiveTime);
    }

    private static void writeCommunityFixture(
            Path path,
            CommunityArtifact communityArtifact,
            byte[] properties,
            byte[] pom,
            boolean includeProperties,
            String extraDescriptor,
            LocalDateTime archiveTime)
            throws IOException {
        byte[] manifest = "Manifest-Version: 1.0\r\n\r\n".getBytes(StandardCharsets.UTF_8);
        try (OutputStream file = Files.newOutputStream(path);
                ZipOutputStream output = new ZipOutputStream(
                        new BufferedOutputStream(file), StandardCharsets.UTF_8)) {
            writeStoredEntry(
                    output, "fixture.txt", "fixture".getBytes(StandardCharsets.UTF_8), archiveTime);
            writeStoredEntry(output, MANIFEST_PATH, manifest, archiveTime);
            writeStoredEntry(
                    output,
                    communityArtifact.descriptorDirectory() + "pom.xml",
                    pom,
                    archiveTime);
            if (includeProperties) {
                writeStoredEntry(
                        output, communityArtifact.propertiesPath(), properties, archiveTime);
            }
            if (extraDescriptor != null) {
                writeStoredEntry(
                        output,
                        extraDescriptor,
                        "unexpected".getBytes(StandardCharsets.UTF_8),
                        archiveTime);
            }
        }
    }

    private static void writeFixture(
            Path path, String signatureName, boolean traversal, LocalDateTime archiveTime)
            throws IOException {
        byte[] manifest = ("Manifest-Version: 1.0\r\n"
                        + "Before: preserved\r\n"
                        + "cLaSs-PaTh: first.jar second-\r\n"
                        + " continued.jar third.jar\r\n"
                        + "After: preserved-too\r\n\r\n")
                .getBytes(StandardCharsets.UTF_8);
        try (OutputStream file = Files.newOutputStream(path);
                ZipOutputStream output = new ZipOutputStream(
                        new BufferedOutputStream(file), StandardCharsets.UTF_8)) {
            writeStoredEntry(
                    output, "z-last.txt", "last".getBytes(StandardCharsets.UTF_8), archiveTime);
            writeStoredEntry(output, MANIFEST_PATH, manifest, archiveTime);
            writeStoredEntry(
                    output, "a-first.txt", "first".getBytes(StandardCharsets.UTF_8), archiveTime);
            if (signatureName != null) {
                writeStoredEntry(
                        output,
                        "META-INF/" + signatureName,
                        "signature".getBytes(StandardCharsets.UTF_8),
                        archiveTime);
            }
            if (traversal) {
                writeStoredEntry(
                        output,
                        "../escape",
                        "escape".getBytes(StandardCharsets.UTF_8),
                        archiveTime);
            }
        }
    }

    private static void verifyFixture(Path artifact, LocalDateTime archiveTime) throws IOException {
        if (hasNonEmptyManifestClassPath(artifact)) {
            throw new IOException("continuation fixture retained Class-Path");
        }
        try (ZipFile jar = new ZipFile(artifact.toFile(), StandardCharsets.UTF_8)) {
            List<String> expectedNames = List.of(MANIFEST_PATH, "a-first.txt", "z-last.txt");
            List<String> actualNames = new ArrayList<>();
            Enumeration<? extends ZipEntry> entries = jar.entries();
            while (entries.hasMoreElements()) {
                ZipEntry entry = entries.nextElement();
                actualNames.add(entry.getName());
                if (entry.getMethod() != ZipEntry.STORED
                        || entry.getExtra() != null
                        || entry.getComment() != null
                        || !entry.getTimeLocal().equals(archiveTime)) {
                    throw new IOException("fixture entry metadata was not canonical");
                }
            }
            if (!actualNames.equals(expectedNames)) {
                throw new IOException("fixture entry order was not canonical: " + actualNames);
            }
            Manifest manifest;
            try (InputStream input = jar.getInputStream(jar.getEntry(MANIFEST_PATH))) {
                manifest = new Manifest(input);
            }
            Attributes attributes = manifest.getMainAttributes();
            if (attributes.getValue(Attributes.Name.CLASS_PATH) != null
                    || !"preserved".equals(attributes.getValue("Before"))
                    || !"preserved-too".equals(attributes.getValue("After"))) {
                throw new IOException("fixture manifest semantics changed unexpectedly");
            }
        }
    }

    private static void expectRejected(
            Path artifact,
            LocalDateTime archiveTime,
            RebuildPlan plan,
            String expectedMessage)
            throws IOException {
        try {
            rebuild(artifact, archiveTime, plan);
        } catch (IOException failure) {
            if (failure.getMessage() != null && failure.getMessage().contains(expectedMessage)) {
                return;
            }
            throw failure;
        }
        throw new IOException("sanitizer accepted an unsafe fixture: " + expectedMessage);
    }

    private static void expectSourceRejected(
            Path artifact, SourceArtifact expected, String expectedMessage) throws IOException {
        try {
            requireExpectedSource(artifact, expected);
        } catch (IOException failure) {
            if (failure.getMessage() != null && failure.getMessage().contains(expectedMessage)) {
                return;
            }
            throw failure;
        }
        throw new IOException("sanitizer accepted source drift: " + expectedMessage);
    }

    private record SourceArtifact(long size, String sha256) {}

    private record CommunityArtifact(String artifactId) {
        private String descriptorDirectory() {
            return COMMUNITY_MAVEN_ROOT + artifactId + "/";
        }

        private String propertiesPath() {
            return descriptorDirectory() + "pom.properties";
        }

        private byte[] properties(String newline) {
            return ("artifactId=" + artifactId + newline
                            + "groupId=" + COMMUNITY_GROUP_ID + newline
                            + "version=" + COMMUNITY_VERSION + newline)
                    .getBytes(StandardCharsets.UTF_8);
        }
    }

    private record RebuildPlan(
            boolean stripManifestClassPath, CommunityArtifact communityArtifact) {
        private static RebuildPlan manifestClassPath() {
            return new RebuildPlan(true, null);
        }

        private static RebuildPlan community(CommunityArtifact communityArtifact) {
            return new RebuildPlan(false, communityArtifact);
        }
    }
}

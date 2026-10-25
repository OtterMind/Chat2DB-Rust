package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.ClientEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ClientHello;
import ai.chat2db.rust.compat.protocol.v1.Ping;
import ai.chat2db.rust.compat.protocol.v1.ProtocolVersion;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.Shutdown;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.jar.JarFile;
import org.junit.jupiter.api.Test;

class ExecutableJarIT {

    private static final Duration TIMEOUT = Duration.ofSeconds(10);
    private static final String H2_DRIVER_ENTRY = "org/h2/Driver.class";
    private static final String SQL_FORMATTER_ENTRY =
            "com/github/vertical_blank/sqlformatter/SqlFormatter.class";

    @Test
    void h2TestDriverRemainsExternalToTheShadedRuntime() throws Exception {
        Path runtimeJar = Path.of(System.getProperty("compat.runtime.jar"));
        Path h2DriverJar = Path.of(System.getProperty("compat.h2.driver.jar"));
        assertTrue(Files.isRegularFile(runtimeJar), "shaded runtime jar must exist");
        assertTrue(Files.isRegularFile(h2DriverJar), "external H2 test driver must exist");

        try (JarFile runtimeArchive = new JarFile(runtimeJar.toFile());
                JarFile h2Archive = new JarFile(h2DriverJar.toFile())) {
            assertFalse(
                    runtimeArchive.stream().anyMatch(entry -> entry.getName().equals(H2_DRIVER_ENTRY)),
                    "shaded runtime jar must not embed the H2 driver");
            assertTrue(
                    h2Archive.stream().anyMatch(entry -> entry.getName().equals(H2_DRIVER_ENTRY)),
                    "external H2 test driver must contain org.h2.Driver");
            assertTrue(
                    runtimeArchive.stream()
                            .anyMatch(entry -> entry.getName().equals(SQL_FORMATTER_ENTRY)),
                    "shaded runtime jar must embed the fixed SQL formatter");
        }
    }


    private static ServerEnvelope readResponse(ExecutorService readers, InputStream input)
            throws Exception {
        Future<ServerEnvelope> response = readers.submit(() ->
                FrameCodec.readFrame(input, ServerEnvelope.parser()).orElseThrow());
        return get(response);
    }

    private static <T> T get(Future<T> future)
            throws InterruptedException, ExecutionException, TimeoutException {
        return future.get(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
    }

    private static ClientEnvelope hello() {
        ClientHello hello = ClientHello.newBuilder()
                .setRuntimeName("chat2db-rust")
                .setRuntimeVersion("integration-test")
                .addSupportedVersions(
                        ProtocolVersion.newBuilder().setMajor(1).setMinor(0))
                .addAllRequiredCapabilities(
                        List.of(ProtocolLoop.PING_CAPABILITY, ProtocolLoop.SHUTDOWN_CAPABILITY))
                .setMaxReceiveFrameBytes(FrameCodec.MAX_FRAME_BYTES)
                .build();
        return ClientEnvelope.newBuilder().setMeta(meta("hello")).setHello(hello).build();
    }

    private static RequestMeta meta(String requestId) {
        return RequestMeta.newBuilder()
                .setRequestId(requestId)
                .setTraceId("trace-" + requestId)
                .build();
    }

    private static String javaExecutable() {
        String executable = System.getProperty("os.name").toLowerCase(Locale.ROOT).contains("win")
                ? "java.exe"
                : "java";
        return Path.of(System.getProperty("java.home"), "bin", executable).toString();
    }

}

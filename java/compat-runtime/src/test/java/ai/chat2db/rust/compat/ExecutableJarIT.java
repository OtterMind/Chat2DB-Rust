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
import org.junit.jupiter.api.Test;

class ExecutableJarIT {

    private static final Duration TIMEOUT = Duration.ofSeconds(10);

    @Test
    void shadedJarRunsTheBinaryProtocolWithoutStdoutContamination() throws Exception {
        Path runtimeJar = Path.of(System.getProperty("compat.runtime.jar"));
        assertTrue(Files.isRegularFile(runtimeJar), "shaded runtime jar must exist");

        Process process = new ProcessBuilder(javaExecutable(), "-jar", runtimeJar.toString()).start();
        ExecutorService readers = Executors.newFixedThreadPool(2);
        Future<String> diagnostics = readers.submit(() -> new String(
                process.getErrorStream().readAllBytes(), StandardCharsets.UTF_8));
        try {
            FrameCodec.writeFrame(process.getOutputStream(), hello());
            ServerEnvelope serverHello = readResponse(readers, process.getInputStream());
            assertEquals(ServerEnvelope.PayloadCase.HELLO, serverHello.getPayloadCase());
            assertEquals(1, serverHello.getHello().getSelectedVersion().getMajor());
            assertEquals(0, serverHello.getHello().getSelectedVersion().getMinor());
            assertEquals(
                    List.of(ProtocolLoop.PING_CAPABILITY, ProtocolLoop.SHUTDOWN_CAPABILITY),
                    serverHello.getHello().getCapabilitiesList());

            FrameCodec.writeFrame(
                    process.getOutputStream(),
                    ClientEnvelope.newBuilder()
                            .setMeta(meta("ping"))
                            .setPing(Ping.newBuilder().setNonce(99))
                            .build());
            ServerEnvelope pong = readResponse(readers, process.getInputStream());
            assertEquals(99, pong.getPong().getNonce());

            FrameCodec.writeFrame(
                    process.getOutputStream(),
                    ClientEnvelope.newBuilder()
                            .setMeta(meta("shutdown"))
                            .setShutdown(Shutdown.newBuilder().setReason("integration test"))
                            .build());
            ServerEnvelope shutdown = readResponse(readers, process.getInputStream());
            assertEquals(ServerEnvelope.PayloadCase.SHUTDOWN_ACK, shutdown.getPayloadCase());

            assertTrue(process.waitFor(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS));
            assertEquals(CompatibilityRuntime.EXIT_OK, process.exitValue());
            assertEquals(-1, process.getInputStream().read(), "stdout must end after the final frame");

            String diagnosticText = get(diagnostics);
            assertTrue(diagnosticText.contains("handshake accepted protocol=1.0"));
            assertTrue(diagnosticText.contains("shutdown accepted request_id=shutdown"));
            assertFalse(diagnosticText.contains("integration test"), "shutdown reason must not be logged");
        } finally {
            if (process.isAlive()) {
                process.destroyForcibly();
                process.waitFor(TIMEOUT.toMillis(), TimeUnit.MILLISECONDS);
            }
            readers.shutdownNow();
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

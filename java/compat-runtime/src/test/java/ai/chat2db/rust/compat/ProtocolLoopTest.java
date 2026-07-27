package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.CancelOperationRequest;
import ai.chat2db.rust.compat.protocol.v1.ClientEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ClientHello;
import ai.chat2db.rust.compat.protocol.v1.CompleteCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.Ping;
import ai.chat2db.rust.compat.protocol.v1.ProtocolVersion;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.Shutdown;
import com.google.protobuf.MessageLite;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.PipedInputStream;
import java.io.PipedOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;
import org.junit.jupiter.api.Test;

class ProtocolLoopTest {

    @Test
    void negotiatesHighestCommonVersionThenPingsAndShutsDown() throws Exception {
        ClientEnvelope hello = hello(
                "hello",
                List.of(version(2, 0), version(1, 0), version(1, 1)),
                List.of(ProtocolLoop.PING_CAPABILITY, ProtocolLoop.SHUTDOWN_CAPABILITY));
        ClientEnvelope ping = ClientEnvelope.newBuilder()
                .setMeta(meta("ping"))
                .setPing(Ping.newBuilder().setNonce(42))
                .build();
        ClientEnvelope shutdown = ClientEnvelope.newBuilder()
                .setMeta(meta("shutdown"))
                .setShutdown(Shutdown.newBuilder().setReason("test complete"))
                .build();

        RunResult result = run(hello, ping, shutdown);

        assertEquals(CompatibilityRuntime.EXIT_OK, result.exitCode());
        assertEquals(3, result.responses().size());
        ServerEnvelope serverHello = result.responses().get(0);
        assertEquals(ServerEnvelope.PayloadCase.HELLO, serverHello.getPayloadCase());
        assertEquals(version(1, 0), serverHello.getHello().getSelectedVersion());
        assertEquals(
                expectedCapabilities(),
                serverHello.getHello().getCapabilitiesList());
        assertEquals(FrameCodec.MAX_FRAME_BYTES, serverHello.getHello().getMaxReceiveFrameBytes());
        assertResponseMeta(serverHello, "hello");

        ServerEnvelope pong = result.responses().get(1);
        assertEquals(42, pong.getPong().getNonce());
        assertEquals(5, pong.getPong().getUptimeMillis());
        assertResponseMeta(pong, "ping");

        ServerEnvelope shutdownAck = result.responses().get(2);
        assertEquals(ServerEnvelope.PayloadCase.SHUTDOWN_ACK, shutdownAck.getPayloadCase());
        assertResponseMeta(shutdownAck, "shutdown");
        assertTrue(result.diagnostics().contains("handshake accepted protocol=1.0"));
        assertTrue(result.diagnostics().contains("shutdown accepted request_id=shutdown"));
    }

    @Test
    void advertisesSqlToolsOnlyWithCommunityCompatibility() {
        assertFalse(ProtocolLoop.capabilities(false)
                .contains(ProtocolLoop.COMMUNITY_SQL_VALIDATION_CAPABILITY));
        assertFalse(ProtocolLoop.capabilities(false)
                .contains(ProtocolLoop.COMMUNITY_SQL_FORMATTER_CAPABILITY));
        assertFalse(ProtocolLoop.capabilities(false)
                .contains(ProtocolLoop.COMMUNITY_SQL_COMPLETION_CAPABILITY));
        assertFalse(ProtocolLoop.capabilities(false)
                .contains(ProtocolLoop.COMMUNITY_DML_BUILDER_CAPABILITY));
        assertTrue(ProtocolLoop.capabilities(true)
                .contains(ProtocolLoop.COMMUNITY_SQL_VALIDATION_CAPABILITY));
        assertTrue(ProtocolLoop.capabilities(true)
                .contains(ProtocolLoop.COMMUNITY_SQL_FORMATTER_CAPABILITY));
        assertTrue(ProtocolLoop.capabilities(true)
                .contains(ProtocolLoop.COMMUNITY_SQL_COMPLETION_CAPABILITY));
        assertTrue(ProtocolLoop.capabilities(true)
                .contains(ProtocolLoop.COMMUNITY_DML_BUILDER_CAPABILITY));
        assertEquals(
                ProtocolLoop.capabilities(false).size() + 4,
                ProtocolLoop.capabilities(true).size());
    }

    @Test
    void dispatchesCommunitySqlCompletionToJdbcRuntime() throws Exception {
        ClientEnvelope completion = ClientEnvelope.newBuilder()
                .setMeta(meta("complete"))
                .setCompleteCommunitySql(CompleteCommunitySqlRequest.newBuilder()
                        .setDatabaseType("H2")
                        .setDatabaseName("catalog")
                        .setSchemaName("PUBLIC")
                        .setDatasourceName("test")
                        .setSql("SELECT ")
                        .setCursorUtf16(7)
                        .setKeywordCase("UPPER")
                        .setDatasourceScope(1))
                .build();
        ClientEnvelope shutdown = ClientEnvelope.newBuilder()
                .setMeta(meta("shutdown"))
                .setShutdown(Shutdown.getDefaultInstance())
                .build();
        ExecutorService executor = Executors.newFixedThreadPool(2);
        try (PipedInputStream runtimeInput = new PipedInputStream(64 * 1024);
                PipedOutputStream clientOutput = new PipedOutputStream(runtimeInput);
                PipedInputStream clientInput = new PipedInputStream(64 * 1024);
                PipedOutputStream runtimeOutput = new PipedOutputStream(clientInput);
                PrintStream diagnostics = new PrintStream(
                        new ByteArrayOutputStream(), true, StandardCharsets.UTF_8)) {
            ProtocolLoop loop = new ProtocolLoop(diagnostics);
            Future<Integer> loopResult =
                    executor.submit(() -> loop.serve(runtimeInput, runtimeOutput));

            FrameCodec.writeFrame(
                    clientOutput,
                    hello(
                            "hello",
                            List.of(version(1, 0)),
                            List.of(ProtocolLoop.SHUTDOWN_CAPABILITY)));
            assertEquals(
                    ServerEnvelope.PayloadCase.HELLO,
                    read(clientInput, executor).getPayloadCase());

            FrameCodec.writeFrame(clientOutput, completion);
            ServerEnvelope completionResponse = read(clientInput, executor);
            assertEquals(ServerEnvelope.PayloadCase.ERROR, completionResponse.getPayloadCase());
            assertEquals("session.id_required", completionResponse.getError().getCode());

            FrameCodec.writeFrame(clientOutput, shutdown);
            assertEquals(
                    ServerEnvelope.PayloadCase.SHUTDOWN_ACK,
                    read(clientInput, executor).getPayloadCase());
            assertEquals(CompatibilityRuntime.EXIT_OK, loopResult.get(5, TimeUnit.SECONDS));
        } finally {
            executor.shutdownNow();
            assertTrue(executor.awaitTermination(5, TimeUnit.SECONDS));
        }
    }

    @Test
    void rejectsIncompatibleVersionsWithAStructuredFatalError() throws Exception {
        RunResult result = run(hello(
                "hello",
                List.of(version(2, 0), version(0, 9)),
                List.of(ProtocolLoop.PING_CAPABILITY)));

        assertEquals(CompatibilityRuntime.EXIT_INCOMPATIBLE, result.exitCode());
        assertEquals(1, result.responses().size());
        ServerEnvelope response = result.responses().get(0);
        assertEquals("protocol.unsupported_version", response.getError().getCode());
        assertTrue(response.getError().getFatal());
        assertEquals("1.0", response.getError().getMetadataOrThrow("supportedVersions"));
        assertResponseMeta(response, "hello");
    }

    @Test
    void rejectsMissingRequiredCapabilitiesWithAStructuredFatalError() throws Exception {
        RunResult result = run(hello(
                "hello",
                List.of(version(1, 0)),
                List.of(ProtocolLoop.PING_CAPABILITY, "database.query.v1")));

        assertEquals(CompatibilityRuntime.EXIT_INCOMPATIBLE, result.exitCode());
        ServerEnvelope response = result.responses().get(0);
        assertEquals("protocol.unsupported_capability", response.getError().getCode());
        assertEquals(
                "database.query.v1",
                response.getError().getMetadataOrThrow("missingCapabilities"));
        assertTrue(response.getError().getFatal());
    }

    @Test
    void requiresHandshakeBeforeLifecycleRequests() throws Exception {
        ClientEnvelope ping = ClientEnvelope.newBuilder()
                .setMeta(meta("early-ping"))
                .setPing(Ping.newBuilder().setNonce(1))
                .build();

        RunResult result = run(ping);

        assertEquals(CompatibilityRuntime.EXIT_PROTOCOL, result.exitCode());
        assertEquals("protocol.handshake_required", result.responses().get(0).getError().getCode());
        assertTrue(result.responses().get(0).getError().getFatal());
    }

    @Test
    void rejectedHandshakeStillHonorsTheClientReceiveLimit() {
        String oversizedCapability = "x".repeat(2_048);
        ClientEnvelope request = hello(
                "hello",
                List.of(version(1, 0)),
                List.of(oversizedCapability),
                1_024);

        FrameCodec.FrameException tooLarge =
                assertThrows(FrameCodec.FrameException.class, () -> run(request));

        assertEquals(FrameCodec.FrameError.TOO_LARGE, tooLarge.reason());
    }

    @Test
    void successfulHandshakeHonorsTheClientReceiveLimit() {
        ClientEnvelope request = hello(
                "hello",
                List.of(version(1, 0)),
                List.of(ProtocolLoop.PING_CAPABILITY),
                1_024);
        RuntimeInfo oversizedRuntime =
                new RuntimeInfo("x".repeat(2_048), "test", 1, 0);

        FrameCodec.FrameException tooLarge = assertThrows(
                FrameCodec.FrameException.class,
                () -> runWithRuntime(oversizedRuntime, "engine-test", request));

        assertEquals(FrameCodec.FrameError.TOO_LARGE, tooLarge.reason());
    }

    @Test
    void unsupportedPayloadAfterHandshakeIsNonFatal() throws Exception {
        ClientEnvelope unsupported =
                ClientEnvelope.newBuilder().setMeta(meta("unknown")).build();
        ClientEnvelope shutdown = ClientEnvelope.newBuilder()
                .setMeta(meta("shutdown"))
                .setShutdown(Shutdown.getDefaultInstance())
                .build();

        RunResult result = run(
                hello(
                        "hello",
                        List.of(version(1, 0)),
                        List.of(ProtocolLoop.SHUTDOWN_CAPABILITY)),
                unsupported,
                shutdown);

        assertEquals(CompatibilityRuntime.EXIT_OK, result.exitCode());
        assertEquals(3, result.responses().size());
        assertEquals("protocol.unsupported_message", result.responses().get(1).getError().getCode());
        assertFalse(result.responses().get(1).getError().getFatal());
        assertEquals(ServerEnvelope.PayloadCase.SHUTDOWN_ACK, result.responses().get(2).getPayloadCase());
    }

    @Test
    void rejectsOversizedCorrelationBeforeDispatchAndFitsThePeerFrame() throws Exception {
        RequestMeta oversized = RequestMeta.newBuilder()
                .setRequestId("oversized-trace")
                .setTraceId("t".repeat(20_000))
                .build();
        ClientEnvelope cancel = ClientEnvelope.newBuilder()
                .setMeta(oversized)
                .setCancelOperation(CancelOperationRequest.newBuilder()
                        .setTargetRequestId("not-active"))
                .build();

        RunResult result = run(
                hello(
                        "hello",
                        List.of(version(1, 0)),
                        List.of(ProtocolLoop.SHUTDOWN_CAPABILITY),
                        1_024),
                cancel);

        assertEquals(CompatibilityRuntime.EXIT_PROTOCOL, result.exitCode());
        ServerEnvelope failure = result.responses().get(1);
        assertEquals("protocol.limit_exceeded", failure.getError().getCode());
        assertTrue(failure.getError().getFatal());
        assertTrue(failure.getSerializedSize() <= 1_024);
        assertTrue(ProtocolLimits.utf8Length(failure.getMeta().getTraceId())
                <= ProtocolLimits.MAX_DRIVER_ID_BYTES);
    }

    @Test
    void maximumCorrelationFitsCancelAckAtTheMinimumPeerFrame() throws Exception {
        String requestId = "r".repeat(ProtocolLimits.MAX_DRIVER_ID_BYTES);
        String traceId = "t".repeat(ProtocolLimits.MAX_DRIVER_ID_BYTES);
        ClientEnvelope cancel = ClientEnvelope.newBuilder()
                .setMeta(RequestMeta.newBuilder()
                        .setRequestId(requestId)
                        .setTraceId(traceId))
                .setCancelOperation(CancelOperationRequest.newBuilder()
                        .setTargetRequestId("not-active"))
                .build();
        ClientEnvelope shutdown = ClientEnvelope.newBuilder()
                .setMeta(meta("shutdown"))
                .setShutdown(Shutdown.getDefaultInstance())
                .build();

        RunResult result = run(
                hello(
                        "hello",
                        List.of(version(1, 0)),
                        List.of(ProtocolLoop.SHUTDOWN_CAPABILITY),
                        1_024),
                cancel,
                shutdown);

        ServerEnvelope cancelled = result.responses().get(1);
        assertEquals(ServerEnvelope.PayloadCase.OPERATION_CANCELLED, cancelled.getPayloadCase());
        assertEquals(
                CancelDisposition.CANCEL_DISPOSITION_UNKNOWN_REQUEST,
                cancelled.getOperationCancelled().getDisposition());
        assertEquals(requestId, cancelled.getMeta().getRequestId());
        assertEquals(traceId, cancelled.getMeta().getTraceId());
        assertTrue(cancelled.getSerializedSize() <= 1_024);
    }

    private static RunResult run(ClientEnvelope... requests) throws Exception {
        return runWithRuntime(
                new RuntimeInfo("chat2db-java-compat", "test", 1, 0),
                "engine-test",
                requests);
    }

    private static List<String> expectedCapabilities() {
        return ProtocolLoop.capabilities(isCommunityCompatibilityConfigured());
    }

    private static boolean isCommunityCompatibilityConfigured() {
        String classpath = System.getenv(CommunityPluginRegistry.CLASSPATH_ENV);
        return classpath != null && !classpath.isBlank();
    }

    private static RunResult runWithRuntime(
            RuntimeInfo runtimeInfo, String engineInstanceId, ClientEnvelope... requests)
            throws Exception {
        ByteArrayOutputStream inputFrames = new ByteArrayOutputStream();
        for (ClientEnvelope request : requests) {
            FrameCodec.writeFrame(inputFrames, request);
        }

        ByteArrayOutputStream protocolOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream diagnosticOutput = new ByteArrayOutputStream();
        AtomicLong nanoTime = new AtomicLong(1_000_000);
        ProtocolLoop loop = new ProtocolLoop(
                runtimeInfo,
                engineInstanceId,
                () -> nanoTime.getAndAdd(5_000_000),
                new PrintStream(diagnosticOutput, true, StandardCharsets.UTF_8));

        int exitCode = loop.serve(new ByteArrayInputStream(inputFrames.toByteArray()), protocolOutput);
        return new RunResult(
                exitCode,
                decodeResponses(protocolOutput.toByteArray()),
                diagnosticOutput.toString(StandardCharsets.UTF_8));
    }

    private static List<ServerEnvelope> decodeResponses(byte[] frames) throws Exception {
        ByteArrayInputStream input = new ByteArrayInputStream(frames);
        List<ServerEnvelope> responses = new ArrayList<>();
        while (true) {
            var response = FrameCodec.readFrame(input, ServerEnvelope.parser());
            if (response.isEmpty()) {
                return responses;
            }
            responses.add(response.orElseThrow());
        }
    }

    private static ServerEnvelope read(PipedInputStream input, ExecutorService executor)
            throws Exception {
        Future<ServerEnvelope> response = executor.submit(
                () -> FrameCodec.readFrame(input, ServerEnvelope.parser()).orElseThrow());
        return response.get(5, TimeUnit.SECONDS);
    }

    private static ClientEnvelope hello(
            String requestId,
            List<ProtocolVersion> versions,
            List<String> requiredCapabilities) {
        return hello(requestId, versions, requiredCapabilities, FrameCodec.MAX_FRAME_BYTES);
    }

    private static ClientEnvelope hello(
            String requestId,
            List<ProtocolVersion> versions,
            List<String> requiredCapabilities,
            int maxReceiveFrameBytes) {
        ClientHello hello = ClientHello.newBuilder()
                .setRuntimeName("chat2db-rust")
                .setRuntimeVersion("test")
                .addAllSupportedVersions(versions)
                .addAllRequiredCapabilities(requiredCapabilities)
                .setMaxReceiveFrameBytes(maxReceiveFrameBytes)
                .build();
        return ClientEnvelope.newBuilder()
                .setMeta(meta(requestId))
                .setHello(hello)
                .build();
    }

    private static RequestMeta meta(String requestId) {
        return RequestMeta.newBuilder()
                .setRequestId(requestId)
                .setTraceId("trace-" + requestId)
                .build();
    }

    private static ProtocolVersion version(int major, int minor) {
        return ProtocolVersion.newBuilder().setMajor(major).setMinor(minor).build();
    }

    private static void assertResponseMeta(ServerEnvelope response, String requestId) {
        assertEquals(requestId, response.getMeta().getRequestId());
        assertEquals("trace-" + requestId, response.getMeta().getTraceId());
        assertEquals(0, response.getMeta().getSequence());
        assertTrue(response.getMeta().getTerminal());
    }

    private record RunResult(int exitCode, List<ServerEnvelope> responses, String diagnostics) {
    }
}

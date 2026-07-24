package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.ClientEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ClientHello;
import ai.chat2db.rust.compat.protocol.v1.EngineError;
import ai.chat2db.rust.compat.protocol.v1.ErrorCategory;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.Pong;
import ai.chat2db.rust.compat.protocol.v1.ProtocolVersion;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ResponseMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import ai.chat2db.rust.compat.protocol.v1.ServerHello;
import ai.chat2db.rust.compat.protocol.v1.ShutdownAck;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.util.Comparator;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Optional;
import java.util.Set;
import java.util.UUID;
import java.util.function.LongSupplier;
import java.util.stream.Collectors;

/** Sequential protocol state machine for the private Java engine process. */
final class ProtocolLoop {

    static final int PROTOCOL_MAJOR = 1;
    static final int PROTOCOL_MINOR = 0;
    static final String PING_CAPABILITY = "lifecycle.ping.v1";
    static final String SHUTDOWN_CAPABILITY = "lifecycle.shutdown.v1";

    private static final int MINIMUM_PEER_FRAME_BYTES = 1024;
    private static final List<String> CAPABILITIES = List.of(PING_CAPABILITY, SHUTDOWN_CAPABILITY);
    private static final List<ProtocolVersion> SUPPORTED_VERSIONS = List.of(version(1, 0));

    private final RuntimeInfo runtimeInfo;
    private final String engineInstanceId;
    private final LongSupplier nanoTime;
    private final long startedAtNanos;
    private final PrintStream diagnostics;

    private State state = State.NEW;
    private int peerMaximumFrameBytes = FrameCodec.MAX_FRAME_BYTES;

    ProtocolLoop(PrintStream diagnostics) {
        this(RuntimeInfo.current(), UUID.randomUUID().toString(), System::nanoTime, diagnostics);
    }

    ProtocolLoop(
            RuntimeInfo runtimeInfo,
            String engineInstanceId,
            LongSupplier nanoTime,
            PrintStream diagnostics) {
        this.runtimeInfo = runtimeInfo;
        this.engineInstanceId = engineInstanceId;
        this.nanoTime = nanoTime;
        this.startedAtNanos = nanoTime.getAsLong();
        this.diagnostics = diagnostics;
    }

    int serve(InputStream input, OutputStream output) throws IOException {
        while (state != State.CLOSED) {
            Optional<ClientEnvelope> request =
                    FrameCodec.readFrame(input, ClientEnvelope.parser());
            if (request.isEmpty()) {
                diagnostics.println("[compat-runtime] stdin closed; stopping protocol loop");
                return CompatibilityRuntime.EXIT_OK;
            }

            Dispatch dispatch = dispatch(request.orElseThrow());
            int maximum = Math.min(peerMaximumFrameBytes, FrameCodec.MAX_FRAME_BYTES);
            FrameCodec.writeFrame(output, dispatch.response(), maximum);
            if (dispatch.terminate()) {
                state = State.CLOSED;
                return dispatch.exitCode();
            }
        }
        return CompatibilityRuntime.EXIT_OK;
    }

    private Dispatch dispatch(ClientEnvelope envelope) {
        RequestMeta meta = envelope.hasMeta()
                ? envelope.getMeta()
                : RequestMeta.getDefaultInstance();
        if (!envelope.hasMeta()
                || meta.getRequestId().isBlank()
                || meta.getTraceId().isBlank()) {
            return error(
                    meta,
                    "protocol.invalid_request_meta",
                    "request_id and trace_id are required",
                    ErrorCategory.ERROR_CATEGORY_VALIDATION,
                    true,
                    CompatibilityRuntime.EXIT_PROTOCOL);
        }

        if (state == State.NEW) {
            if (envelope.getPayloadCase() != ClientEnvelope.PayloadCase.HELLO) {
                return error(
                        meta,
                        "protocol.handshake_required",
                        "the first request must be a client hello",
                        ErrorCategory.ERROR_CATEGORY_PROTOCOL,
                        true,
                        CompatibilityRuntime.EXIT_PROTOCOL);
            }
            return handshake(meta, envelope.getHello());
        }

        return switch (envelope.getPayloadCase()) {
            case PING -> pong(meta, envelope.getPing().getNonce());
            case SHUTDOWN -> shutdown(meta);
            case HELLO -> error(
                    meta,
                    "protocol.handshake_already_completed",
                    "client hello is only valid as the first request",
                    ErrorCategory.ERROR_CATEGORY_PROTOCOL,
                    true,
                    CompatibilityRuntime.EXIT_PROTOCOL);
            case PAYLOAD_NOT_SET -> error(
                    meta,
                    "protocol.unsupported_message",
                    "the request does not contain a supported payload",
                    ErrorCategory.ERROR_CATEGORY_PROTOCOL,
                    false,
                    CompatibilityRuntime.EXIT_OK);
        };
    }

    private Dispatch handshake(RequestMeta meta, ClientHello hello) {
        if (hello.getMaxReceiveFrameBytes() < MINIMUM_PEER_FRAME_BYTES) {
            return error(
                    meta,
                    "protocol.invalid_max_receive_frame_bytes",
                    "max_receive_frame_bytes must be at least " + MINIMUM_PEER_FRAME_BYTES,
                    ErrorCategory.ERROR_CATEGORY_VALIDATION,
                    true,
                    CompatibilityRuntime.EXIT_PROTOCOL);
        }
        peerMaximumFrameBytes = hello.getMaxReceiveFrameBytes();

        Optional<ProtocolVersion> selectedVersion = SUPPORTED_VERSIONS.stream()
                .filter(supported -> hello.getSupportedVersionsList().stream()
                        .anyMatch(requested -> sameVersion(supported, requested)))
                .max(Comparator.comparingInt(ProtocolVersion::getMajor)
                        .thenComparingInt(ProtocolVersion::getMinor));
        if (selectedVersion.isEmpty()) {
            return error(
                    meta,
                    "protocol.unsupported_version",
                    "client and engine do not share a protocol version",
                    ErrorCategory.ERROR_CATEGORY_PROTOCOL,
                    true,
                    CompatibilityRuntime.EXIT_INCOMPATIBLE,
                    "supportedVersions",
                    versionsDisplay(SUPPORTED_VERSIONS));
        }

        Set<String> missingCapabilities = new LinkedHashSet<>(hello.getRequiredCapabilitiesList());
        missingCapabilities.removeAll(CAPABILITIES);
        if (!missingCapabilities.isEmpty()) {
            return error(
                    meta,
                    "protocol.unsupported_capability",
                    "the engine does not provide every required capability",
                    ErrorCategory.ERROR_CATEGORY_PROTOCOL,
                    true,
                    CompatibilityRuntime.EXIT_INCOMPATIBLE,
                    "missingCapabilities",
                    String.join(",", missingCapabilities));
        }

        state = State.READY;
        ProtocolVersion negotiated = selectedVersion.orElseThrow();
        diagnostics.printf(
                "[compat-runtime] handshake accepted protocol=%d.%d%n",
                negotiated.getMajor(),
                negotiated.getMinor());

        ServerHello response = ServerHello.newBuilder()
                .setEngineName(runtimeInfo.name())
                .setEngineVersion(runtimeInfo.version())
                .setEngineInstanceId(engineInstanceId)
                .setSelectedVersion(negotiated)
                .addAllCapabilities(CAPABILITIES)
                .setMaxReceiveFrameBytes(FrameCodec.MAX_FRAME_BYTES)
                .build();
        return new Dispatch(
                response(meta).setHello(response).build(),
                false,
                CompatibilityRuntime.EXIT_OK);
    }

    private Dispatch pong(RequestMeta meta, long nonce) {
        long elapsedNanos = Math.max(0, nanoTime.getAsLong() - startedAtNanos);
        Pong response = Pong.newBuilder()
                .setNonce(nonce)
                .setUptimeMillis(elapsedNanos / 1_000_000)
                .build();
        return new Dispatch(
                response(meta).setPong(response).build(),
                false,
                CompatibilityRuntime.EXIT_OK);
    }

    private Dispatch shutdown(RequestMeta meta) {
        diagnostics.printf(
                "[compat-runtime] shutdown accepted request_id=%s%n", meta.getRequestId());
        return new Dispatch(
                response(meta).setShutdownAck(ShutdownAck.getDefaultInstance()).build(),
                true,
                CompatibilityRuntime.EXIT_OK);
    }

    private Dispatch error(
            RequestMeta meta,
            String code,
            String message,
            ErrorCategory category,
            boolean fatal,
            int exitCode) {
        return error(meta, code, message, category, fatal, exitCode, null, null);
    }

    private Dispatch error(
            RequestMeta meta,
            String code,
            String message,
            ErrorCategory category,
            boolean fatal,
            int exitCode,
            String metadataKey,
            String metadataValue) {
        EngineError.Builder protocolError = EngineError.newBuilder()
                .setCode(code)
                .setMessage(message)
                .setCategory(category)
                .setRetryable(false)
                .setFatal(fatal)
                .setOutcome(OperationOutcome.OPERATION_OUTCOME_NOT_APPLICABLE);
        if (metadataKey != null) {
            protocolError.putMetadata(metadataKey, metadataValue);
        }
        diagnostics.printf("[compat-runtime] protocol error code=%s fatal=%s%n", code, fatal);
        return new Dispatch(
                response(meta).setError(protocolError).build(),
                fatal,
                exitCode);
    }

    private static ServerEnvelope.Builder response(RequestMeta requestMeta) {
        ResponseMeta meta = ResponseMeta.newBuilder()
                .setRequestId(requestMeta.getRequestId())
                .setTraceId(requestMeta.getTraceId())
                .setSequence(0)
                .setTerminal(true)
                .build();
        return ServerEnvelope.newBuilder().setMeta(meta);
    }

    private static ProtocolVersion version(int major, int minor) {
        return ProtocolVersion.newBuilder().setMajor(major).setMinor(minor).build();
    }

    private static boolean sameVersion(ProtocolVersion left, ProtocolVersion right) {
        return left.getMajor() == right.getMajor() && left.getMinor() == right.getMinor();
    }

    private static String versionsDisplay(List<ProtocolVersion> versions) {
        return versions.stream()
                .map(version -> version.getMajor() + "." + version.getMinor())
                .collect(Collectors.joining(","));
    }

    private enum State {
        NEW,
        READY,
        CLOSED
    }

    private record Dispatch(ServerEnvelope response, boolean terminate, int exitCode) {
    }
}

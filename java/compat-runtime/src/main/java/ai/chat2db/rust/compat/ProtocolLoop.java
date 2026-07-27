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
import java.util.ArrayList;
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
    static final String EXTERNAL_DRIVER_CAPABILITY = "driver.external-jar.v1";
    static final String JDBC_SESSION_CAPABILITY = "session.jdbc.v1";
    static final String TYPED_QUERY_CAPABILITY = "query.typed-batches.v1";
    static final String CREDIT_FLOW_CAPABILITY = "flow.credit.v1";
    static final String OPERATION_CANCEL_CAPABILITY = "operation.cancel.v1";
    static final String JDBC_UPDATE_CAPABILITY = "update.jdbc.v1";
    static final String LOCAL_TRANSACTION_CAPABILITY = "transaction.local.v1";
    static final String COMMUNITY_PLUGIN_CATALOG_CAPABILITY = "community.plugin-catalog.v1";
    static final String COMMUNITY_SCHEMA_METADATA_CAPABILITY = "community.metadata.schemas.v1";
    static final String COMMUNITY_OBJECT_METADATA_CAPABILITY = "community.metadata.objects.v1";
    static final String COMMUNITY_RELATION_METADATA_CAPABILITY = "community.metadata.relations.v1";
    static final String COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY =
            "community.metadata.programmability.v1";
    static final String COMMUNITY_SQL_BUILDER_CAPABILITY = "community.sql-builder.v1";
    static final String COMMUNITY_SQL_PARSER_CAPABILITY = "community.sql-parser.v1";
    static final String COMMUNITY_SQL_VALIDATION_CAPABILITY = "community.sql-validation.v1";
    static final String COMMUNITY_SQL_FORMATTER_CAPABILITY = "community.sql-formatter.v1";
    static final String COMMUNITY_SQL_COMPLETION_CAPABILITY = "community.sql-completion.v1";

    private static final int MINIMUM_PEER_FRAME_BYTES = 1024;
    private static final List<String> BASE_CAPABILITIES = List.of(
            PING_CAPABILITY,
            SHUTDOWN_CAPABILITY,
            EXTERNAL_DRIVER_CAPABILITY,
            JDBC_SESSION_CAPABILITY,
            TYPED_QUERY_CAPABILITY,
            CREDIT_FLOW_CAPABILITY,
            OPERATION_CANCEL_CAPABILITY,
            JDBC_UPDATE_CAPABILITY,
            LOCAL_TRANSACTION_CAPABILITY,
            COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
            COMMUNITY_SCHEMA_METADATA_CAPABILITY,
            COMMUNITY_OBJECT_METADATA_CAPABILITY,
            COMMUNITY_RELATION_METADATA_CAPABILITY,
            COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
            COMMUNITY_SQL_BUILDER_CAPABILITY,
            COMMUNITY_SQL_PARSER_CAPABILITY);
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
        try (ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime jdbcRuntime = new JdbcRuntime(writer, diagnostics)) {
            while (state != State.CLOSED) {
                Optional<ClientEnvelope> request =
                        FrameCodec.readFrame(input, ClientEnvelope.parser());
                if (request.isEmpty()) {
                    diagnostics.println("[compat-runtime] stdin closed; stopping protocol loop");
                    return CompatibilityRuntime.EXIT_OK;
                }

                Dispatch dispatch = dispatch(request.orElseThrow(), jdbcRuntime, writer);
                if (dispatch.terminate()) {
                    jdbcRuntime.close();
                }
                if (dispatch.response() != null) {
                    writer.write(dispatch.response());
                }
                if (dispatch.terminate()) {
                    state = State.CLOSED;
                    return dispatch.exitCode();
                }
            }
        }
        return CompatibilityRuntime.EXIT_OK;
    }

    private Dispatch dispatch(
            ClientEnvelope envelope, JdbcRuntime jdbcRuntime, ProtocolWriter writer) {
        RequestMeta meta = envelope.hasMeta()
                ? envelope.getMeta()
                : RequestMeta.getDefaultInstance();
        if (!envelope.hasMeta()
                || meta.getRequestId().isBlank()
                || meta.getTraceId().isBlank()) {
            return error(
                    compactCorrelationMeta(meta),
                    "protocol.invalid_request_meta",
                    "request_id and trace_id are required",
                    ErrorCategory.ERROR_CATEGORY_VALIDATION,
                    true,
                    CompatibilityRuntime.EXIT_PROTOCOL);
        }
        try {
            validateRequestMeta(meta);
        } catch (RuntimeFailure failure) {
            return error(
                    compactCorrelationMeta(meta),
                    failure.code(),
                    failure.getMessage(),
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
            return handshake(meta, envelope.getHello(), writer, jdbcRuntime);
        }

        try {
            return switch (envelope.getPayloadCase()) {
                case PING -> pong(meta, envelope.getPing().getNonce());
                case SHUTDOWN -> shutdown(meta);
                case LOAD_DRIVER -> {
                    jdbcRuntime.schedule(
                            meta, () -> jdbcRuntime.loadDriver(meta, envelope.getLoadDriver()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case UNLOAD_DRIVER -> {
                    jdbcRuntime.schedule(
                            meta, () -> jdbcRuntime.unloadDriver(meta, envelope.getUnloadDriver()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case OPEN_SESSION -> {
                    jdbcRuntime.schedule(
                            meta, () -> jdbcRuntime.openSession(meta, envelope.getOpenSession()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case CLOSE_SESSION -> {
                    jdbcRuntime.schedule(
                            meta, () -> jdbcRuntime.closeSession(meta, envelope.getCloseSession()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case BEGIN_TRANSACTION -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.beginTransaction(meta, envelope.getBeginTransaction()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case COMMIT_TRANSACTION -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.commitTransaction(meta, envelope.getCommitTransaction()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case ROLLBACK_TRANSACTION -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.rollbackTransaction(meta, envelope.getRollbackTransaction()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case EXECUTE_QUERY -> {
                    jdbcRuntime.executeQuery(meta, envelope.getExecuteQuery());
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case EXECUTE_UPDATE -> {
                    jdbcRuntime.executeUpdate(meta, envelope.getExecuteUpdate());
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case GRANT_CREDITS -> response(jdbcRuntime.grantCredits(
                        meta, envelope.getGrantCredits()));
                case CANCEL_OPERATION -> response(jdbcRuntime.cancelOperation(
                        meta, envelope.getCancelOperation()));
                case LIST_COMMUNITY_PLUGINS -> {
                    jdbcRuntime.schedule(meta, () -> jdbcRuntime.listCommunityPlugins(meta));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_SCHEMAS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunitySchemas(
                                    meta, envelope.getListCommunitySchemas()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_DATABASES -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityDatabases(
                                    meta, envelope.getListCommunityDatabases()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_TABLES -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityTables(
                                    meta, envelope.getListCommunityTables()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_COLUMNS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityColumns(
                                    meta, envelope.getListCommunityColumns()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_INDEXES -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityIndexes(
                                    meta, envelope.getListCommunityIndexes()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_VIEWS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityViews(
                                    meta, envelope.getListCommunityViews()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_IMPORTED_KEYS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityImportedKeys(
                                    meta, envelope.getListCommunityImportedKeys()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_EXPORTED_KEYS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityExportedKeys(
                                    meta, envelope.getListCommunityExportedKeys()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_PRIMARY_KEYS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityPrimaryKeys(
                                    meta, envelope.getListCommunityPrimaryKeys()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_FUNCTIONS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityFunctions(
                                    meta, envelope.getListCommunityFunctions()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case GET_COMMUNITY_FUNCTION -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.getCommunityFunction(
                                    meta, envelope.getGetCommunityFunction()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_FUNCTION_PARAMETERS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityFunctionParameters(
                                    meta, envelope.getListCommunityFunctionParameters()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_PROCEDURES -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityProcedures(
                                    meta, envelope.getListCommunityProcedures()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case GET_COMMUNITY_PROCEDURE -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.getCommunityProcedure(
                                    meta, envelope.getGetCommunityProcedure()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_PROCEDURE_PARAMETERS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityProcedureParameters(
                                    meta, envelope.getListCommunityProcedureParameters()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case LIST_COMMUNITY_TRIGGERS -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.listCommunityTriggers(
                                    meta, envelope.getListCommunityTriggers()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case GET_COMMUNITY_TRIGGER -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.getCommunityTrigger(
                                    meta, envelope.getGetCommunityTrigger()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case BUILD_COMMUNITY_CREATE_SCHEMA -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.buildCommunityCreateSchema(
                                    meta, envelope.getBuildCommunityCreateSchema()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case PARSE_COMMUNITY_SQL -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.parseCommunitySql(
                                    meta, envelope.getParseCommunitySql()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case VALIDATE_COMMUNITY_SQL -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.validateCommunitySql(
                                    meta, envelope.getValidateCommunitySql()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case FORMAT_COMMUNITY_SQL -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.formatCommunitySql(
                                    meta, envelope.getFormatCommunitySql()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
                case COMPLETE_COMMUNITY_SQL -> {
                    jdbcRuntime.schedule(
                            meta,
                            () -> jdbcRuntime.completeCommunitySql(
                                    meta, envelope.getCompleteCommunitySql()));
                    yield new Dispatch(null, false, CompatibilityRuntime.EXIT_OK);
                }
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
        } catch (RuntimeFailure failure) {
            failure = jdbcRuntime.attachSessionState(meta, failure);
            diagnostics.printf(
                    "[compat-runtime] JDBC request failed code=%s request_id=%s%n",
                    failure.code(),
                    meta.getRequestId());
            return response(ProtocolResponses.failure(
                    meta, 0, failure, writer.peerMaximumFrameBytes()));
        }
    }

    private Dispatch handshake(
            RequestMeta meta,
            ClientHello hello,
            ProtocolWriter writer,
            JdbcRuntime jdbcRuntime) {
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
        writer.setPeerMaximumFrameBytes(peerMaximumFrameBytes);

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

        List<String> capabilities =
                capabilities(jdbcRuntime.communityCompatibilityConfigured());
        Set<String> missingCapabilities = new LinkedHashSet<>(hello.getRequiredCapabilitiesList());
        missingCapabilities.removeAll(capabilities);
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
                .addAllCapabilities(capabilities)
                .setMaxReceiveFrameBytes(FrameCodec.MAX_FRAME_BYTES)
                .build();
        return new Dispatch(
                response(meta).setHello(response).build(),
                false,
                CompatibilityRuntime.EXIT_OK);
    }

    static List<String> capabilities(boolean communityCompatibilityConfigured) {
        if (!communityCompatibilityConfigured) {
            return BASE_CAPABILITIES;
        }
        List<String> capabilities = new ArrayList<>(BASE_CAPABILITIES);
        capabilities.add(COMMUNITY_SQL_VALIDATION_CAPABILITY);
        capabilities.add(COMMUNITY_SQL_FORMATTER_CAPABILITY);
        capabilities.add(COMMUNITY_SQL_COMPLETION_CAPABILITY);
        return List.copyOf(capabilities);
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

    private static Dispatch response(ServerEnvelope response) {
        return new Dispatch(response, false, CompatibilityRuntime.EXIT_OK);
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

    private static void validateRequestMeta(RequestMeta meta) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                meta.getRequestId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "request_id");
        ProtocolLimits.requireNonBlankUtf8(
                meta.getTraceId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "trace_id");
        if (meta.hasSessionId()) {
            ProtocolLimits.requireNonBlankUtf8(
                    meta.getSessionId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "session_id");
        }
        if (meta.hasCancellationId()) {
            ProtocolLimits.requireNonBlankUtf8(
                    meta.getCancellationId(),
                    ProtocolLimits.MAX_DRIVER_ID_BYTES,
                    "cancellation_id");
        }
    }

    private static RequestMeta compactCorrelationMeta(RequestMeta meta) {
        return meta.toBuilder()
                .setRequestId(ProtocolLimits.truncateUtf8(
                        meta.getRequestId(), ProtocolLimits.MAX_DRIVER_ID_BYTES))
                .setTraceId(ProtocolLimits.truncateUtf8(
                        meta.getTraceId(), ProtocolLimits.MAX_DRIVER_ID_BYTES))
                .build();
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

package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.Pong;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.sql.SQLException;
import java.util.List;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;

class ProtocolSafetyTest {

    private static final String SECRET = "sensitive-property-value";

    @Test
    void compactFailureFitsPeerFrameAndOversizedPreflightDoesNotPoisonWriter()
            throws Exception {
        SQLException databaseCause = new SQLException(SECRET + "-" + "x".repeat(20_000));
        RuntimeFailure failure = RuntimeFailure.database(
                        "database.test_failure",
                        "database failed with " + SECRET,
                        databaseCause,
                        OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                        false)
                .withRedactor(new SensitiveDataRedactor(List.of(SECRET)));
        ServerEnvelope compact = ProtocolResponses.failure(meta("failure"), 0, failure, 1024);
        assertTrue(compact.getSerializedSize() <= 1024);
        assertFalse(compact.toString().contains(SECRET));
        ServerEnvelope compactCorrelation = ProtocolResponses.failure(
                meta("failure").toBuilder().setTraceId("t".repeat(20_000)).build(),
                0,
                failure,
                1024);
        assertTrue(compactCorrelation.getSerializedSize() <= 1024);
        assertEquals("failure", compactCorrelation.getMeta().getRequestId());

        ByteArrayOutputStream output = new ByteArrayOutputStream();
        try (ProtocolWriter writer = new ProtocolWriter(output)) {
            writer.setPeerMaximumFrameBytes(1024);
            ServerEnvelope oversized = ProtocolResponses.response(meta("large"), 0, true)
                    .setError(ai.chat2db.rust.compat.protocol.v1.EngineError.newBuilder()
                            .setCode("protocol.large")
                            .setMessage("x".repeat(2048)))
                    .build();
            assertThrows(FrameCodec.FrameException.class, () -> writer.write(oversized));

            ServerEnvelope small = ProtocolResponses.response(meta("small"), 0, true)
                    .setPong(Pong.newBuilder().setNonce(7))
                    .build();
            writer.write(small);
        }
        ServerEnvelope written = FrameCodec.readFrame(
                        new ByteArrayInputStream(output.toByteArray()), ServerEnvelope.parser())
                .orElseThrow();
        assertEquals("small", written.getMeta().getRequestId());
        assertEquals(7, written.getPong().getNonce());
    }

    @Test
    void sensitiveConnectionValueIsAbsentFromAsyncWireAndDiagnostics() throws Exception {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        ByteArrayOutputStream diagnostics = new ByteArrayOutputStream();
        try (ProtocolWriter writer = new ProtocolWriter(output);
                JdbcRuntime runtime = new JdbcRuntime(
                        writer,
                        new PrintStream(diagnostics, true, StandardCharsets.UTF_8))) {
            RuntimeFailure failure = RuntimeFailure.database(
                            "session.open_failed",
                            "open failed",
                            new SQLException("driver echoed " + SECRET),
                            OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                            false)
                    .withRedactor(new SensitiveDataRedactor(List.of(SECRET)));
            runtime.schedule(meta("sensitive"), () -> {
                throw failure;
            });
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
            while (output.size() == 0 && System.nanoTime() < deadline) {
                TimeUnit.MILLISECONDS.sleep(5);
            }
            assertTrue(output.size() > 0);
        }

        byte[] wire = output.toByteArray();
        ServerEnvelope response = FrameCodec.readFrame(
                        new ByteArrayInputStream(wire), ServerEnvelope.parser())
                .orElseThrow();
        assertEquals(ServerEnvelope.PayloadCase.ERROR, response.getPayloadCase());
        assertFalse(response.toString().contains(SECRET));
        assertFalse(new String(wire, StandardCharsets.ISO_8859_1).contains(SECRET));
        assertFalse(diagnostics.toString(StandardCharsets.UTF_8).contains(SECRET));
    }

    @Test
    void hugeSqlExceptionMessageRedactsSecretAcrossTheTruncationBoundary() {
        String prefix = "x".repeat(2045);
        SQLException databaseCause = new SQLException(
                prefix + SECRET + "y".repeat(8 * 1024 * 1024));
        RuntimeFailure failure = RuntimeFailure.database(
                        "database.test_failure",
                        "database failed",
                        databaseCause,
                        OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                        false)
                .withRedactor(new SensitiveDataRedactor(List.of(SECRET)));

        String causeMessage = failure.toEngineError()
                .getDatabaseError()
                .getCauses(0)
                .getMessage();
        assertEquals(prefix + "[RE", causeMessage);
        assertEquals(2048, ProtocolLimits.utf8Length(causeMessage));
        assertFalse(causeMessage.contains(SECRET));
    }

    @Test
    void boundedRedactionDoesNotScanAnOversizedInputTail() {
        TrackingCharSequence input = new TrackingCharSequence(100_000_000);

        String result = SensitiveDataRedactor.NONE.redactAndTruncate(input, 64);

        assertEquals("x".repeat(64), result);
        assertTrue(input.highestReadIndex() < 64);
    }

    private static RequestMeta meta(String requestId) {
        return RequestMeta.newBuilder()
                .setRequestId(requestId)
                .setTraceId("trace-" + requestId)
                .build();
    }

    private static final class TrackingCharSequence implements CharSequence {
        private final int length;
        private int highestReadIndex = -1;

        private TrackingCharSequence(int length) {
            this.length = length;
        }

        @Override
        public int length() {
            return length;
        }

        @Override
        public char charAt(int index) {
            highestReadIndex = Math.max(highestReadIndex, index);
            return 'x';
        }

        @Override
        public CharSequence subSequence(int start, int end) {
            throw new AssertionError("redaction must not materialize an input slice");
        }

        private int highestReadIndex() {
            return highestReadIndex;
        }
    }
}

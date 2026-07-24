package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.EngineError;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import ai.chat2db.rust.compat.protocol.v1.ResponseMeta;
import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;

final class ProtocolResponses {

    private ProtocolResponses() {
    }

    static ServerEnvelope.Builder response(
            RequestMeta requestMeta, long sequence, boolean terminal) {
        ResponseMeta meta = ResponseMeta.newBuilder()
                .setRequestId(requestMeta.getRequestId())
                .setTraceId(requestMeta.getTraceId())
                .setSequence(sequence)
                .setTerminal(terminal)
                .build();
        return ServerEnvelope.newBuilder().setMeta(meta);
    }

    static ServerEnvelope failure(RequestMeta requestMeta, long sequence, RuntimeFailure failure) {
        return failure(requestMeta, sequence, failure, FrameCodec.MAX_FRAME_BYTES);
    }

    static ServerEnvelope failure(
            RequestMeta requestMeta,
            long sequence,
            RuntimeFailure failure,
            int maximumFrameBytes) {
        ServerEnvelope full = response(requestMeta, sequence, true)
                .setError(failure.toEngineError())
                .build();
        if (full.getSerializedSize() <= maximumFrameBytes) {
            return full;
        }

        EngineError original = full.getError();
        EngineError.Builder compactError = original.toBuilder()
                .clearDatabaseError()
                .clearMetadata()
                .setMessage(ProtocolLimits.truncateUtf8(original.getMessage(), 128));
        ServerEnvelope compact = response(requestMeta, sequence, true)
                .setError(compactError)
                .build();
        if (compact.getSerializedSize() <= maximumFrameBytes) {
            return compact;
        }
        compact = response(requestMeta, sequence, true)
                .setError(compactError.setMessage("request failed; inspect error code"))
                .build();
        if (compact.getSerializedSize() <= maximumFrameBytes) {
            return compact;
        }
        RequestMeta compactMeta = requestMeta.toBuilder()
                .setRequestId(ProtocolLimits.truncateUtf8(
                        requestMeta.getRequestId(), ProtocolLimits.MAX_DRIVER_ID_BYTES))
                .setTraceId(ProtocolLimits.truncateUtf8(
                        requestMeta.getTraceId(), ProtocolLimits.MAX_DRIVER_ID_BYTES))
                .build();
        return response(compactMeta, sequence, true)
                .setError(compactError.clearMessage())
                .build();
    }
}

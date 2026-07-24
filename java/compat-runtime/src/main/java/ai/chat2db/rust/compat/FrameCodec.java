package ai.chat2db.rust.compat;

import com.google.protobuf.InvalidProtocolBufferException;
import com.google.protobuf.MessageLite;
import com.google.protobuf.Parser;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Optional;

/** Four-byte big-endian framing for the private Protobuf process protocol. */
final class FrameCodec {

    static final int MAX_FRAME_BYTES = 16 * 1024 * 1024;

    private FrameCodec() {
    }

    static <T extends MessageLite> Optional<T> readFrame(InputStream input, Parser<T> parser)
            throws IOException {
        int firstHeaderByte = input.read();
        if (firstHeaderByte == -1) {
            return Optional.empty();
        }

        byte[] header = new byte[4];
        header[0] = (byte) firstHeaderByte;
        readFully(input, header, 1, 3, "frame header");

        long payloadLength = ((header[0] & 0xffL) << 24)
                | ((header[1] & 0xffL) << 16)
                | ((header[2] & 0xffL) << 8)
                | (header[3] & 0xffL);
        int validatedLength = validatePayloadLength(payloadLength, MAX_FRAME_BYTES);

        byte[] payload = new byte[validatedLength];
        readFully(input, payload, 0, validatedLength, "frame payload");
        try {
            return Optional.of(parser.parseFrom(payload));
        } catch (InvalidProtocolBufferException exception) {
            throw new FrameException(
                    FrameError.MALFORMED,
                    "frame payload is not a valid Protobuf message",
                    exception);
        }
    }

    static void writeFrame(OutputStream output, MessageLite message) throws IOException {
        writeFrame(output, message, MAX_FRAME_BYTES);
    }

    static void writeFrame(OutputStream output, MessageLite message, int peerMaximum) throws IOException {
        int maximum = Math.min(MAX_FRAME_BYTES, peerMaximum);
        int payloadLength = validatePayloadLength(message.getSerializedSize(), maximum);
        byte[] payload = message.toByteArray();

        output.write((payloadLength >>> 24) & 0xff);
        output.write((payloadLength >>> 16) & 0xff);
        output.write((payloadLength >>> 8) & 0xff);
        output.write(payloadLength & 0xff);
        output.write(payload);
        output.flush();
    }

    private static int validatePayloadLength(long payloadLength, int maximum) throws FrameException {
        if (payloadLength == 0) {
            throw new FrameException(FrameError.EMPTY, "frame payload cannot be empty");
        }
        if (maximum <= 0 || payloadLength > maximum) {
            throw new FrameException(
                    FrameError.TOO_LARGE,
                    "frame payload is " + payloadLength + " bytes; maximum is " + maximum);
        }
        return (int) payloadLength;
    }

    private static void readFully(
            InputStream input, byte[] target, int offset, int length, String description)
            throws IOException {
        int totalRead = 0;
        while (totalRead < length) {
            int bytesRead = input.read(target, offset + totalRead, length - totalRead);
            if (bytesRead == -1) {
                throw new EOFException(description + " was truncated");
            }
            if (bytesRead == 0) {
                int singleByte = input.read();
                if (singleByte == -1) {
                    throw new EOFException(description + " was truncated");
                }
                target[offset + totalRead] = (byte) singleByte;
                totalRead++;
            } else {
                totalRead += bytesRead;
            }
        }
    }

    enum FrameError {
        EMPTY,
        TOO_LARGE,
        MALFORMED
    }

    static final class FrameException extends IOException {
        private final FrameError reason;

        FrameException(FrameError reason, String message) {
            super(message);
            this.reason = reason;
        }

        FrameException(FrameError reason, String message, Throwable cause) {
            super(message, cause);
            this.reason = reason;
        }

        FrameError reason() {
            return reason;
        }
    }
}

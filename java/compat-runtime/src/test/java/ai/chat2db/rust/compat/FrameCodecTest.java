package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.Ping;
import com.google.protobuf.MessageLite;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.EOFException;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicBoolean;
import org.junit.jupiter.api.Test;

class FrameCodecTest {

    @Test
    void framesRoundTripWithABigEndianLengthPrefix() throws Exception {
        Ping ping = Ping.newBuilder().setNonce(300).build();
        byte[] payload = ping.toByteArray();
        ByteArrayOutputStream output = new ByteArrayOutputStream();

        FrameCodec.writeFrame(output, ping);

        byte[] frame = output.toByteArray();
        assertArrayEquals(ByteBuffer.allocate(4).putInt(payload.length).array(), Arrays.copyOf(frame, 4));
        assertArrayEquals(payload, Arrays.copyOfRange(frame, 4, frame.length));
        assertEquals(
                ping,
                FrameCodec.readFrame(new ByteArrayInputStream(frame), Ping.parser())
                        .orElseThrow());
    }

    @Test
    void cleanEofIsDistinctFromTruncatedFrames() throws Exception {
        assertTrue(FrameCodec.readFrame(new ByteArrayInputStream(new byte[0]), Ping.parser()).isEmpty());

        assertThrows(
                EOFException.class,
                () -> FrameCodec.readFrame(new ByteArrayInputStream(new byte[] {0, 0}), Ping.parser()));
        assertThrows(
                EOFException.class,
                () -> FrameCodec.readFrame(
                        new ByteArrayInputStream(new byte[] {0, 0, 0, 2, 8}), Ping.parser()));
    }

    @Test
    void emptyAndOversizedFramesAreRejectedBeforePayloadAllocation() {
        FrameCodec.FrameException empty = assertThrows(
                FrameCodec.FrameException.class,
                () -> FrameCodec.readFrame(
                        new ByteArrayInputStream(new byte[] {0, 0, 0, 0}), Ping.parser()));
        assertEquals(FrameCodec.FrameError.EMPTY, empty.reason());

        byte[] oversized = ByteBuffer.allocate(4)
                .putInt(FrameCodec.MAX_FRAME_BYTES + 1)
                .array();
        FrameCodec.FrameException tooLarge = assertThrows(
                FrameCodec.FrameException.class,
                () -> FrameCodec.readFrame(new ByteArrayInputStream(oversized), Ping.parser()));
        assertEquals(FrameCodec.FrameError.TOO_LARGE, tooLarge.reason());
    }

    @Test
    void malformedProtobufAndPeerLimitsAreRejected() {
        FrameCodec.FrameException malformed = assertThrows(
                FrameCodec.FrameException.class,
                () -> FrameCodec.readFrame(
                        new ByteArrayInputStream(new byte[] {0, 0, 0, 1, (byte) 0x80}),
                        Ping.parser()));
        assertEquals(FrameCodec.FrameError.MALFORMED, malformed.reason());

        FrameCodec.FrameException tooLarge = assertThrows(
                FrameCodec.FrameException.class,
                () -> FrameCodec.writeFrame(
                        new ByteArrayOutputStream(),
                        Ping.newBuilder().setNonce(300).build(),
                        1));
        assertEquals(FrameCodec.FrameError.TOO_LARGE, tooLarge.reason());
    }

    @Test
    void peerLimitIsCheckedFromSerializedSizeBeforeWriting() {
        AtomicBoolean serialized = new AtomicBoolean();
        MessageLite oversized = (MessageLite) Proxy.newProxyInstance(
                MessageLite.class.getClassLoader(),
                new Class<?>[] {MessageLite.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getSerializedSize" -> 2_048;
                    case "toByteArray" -> {
                        serialized.set(true);
                        yield new byte[2_048];
                    }
                    default -> throw new AssertionError("unexpected method: " + method.getName());
                });
        ByteArrayOutputStream output = new ByteArrayOutputStream();

        FrameCodec.FrameException tooLarge = assertThrows(
                FrameCodec.FrameException.class,
                () -> FrameCodec.writeFrame(output, oversized, 1_024));

        assertEquals(FrameCodec.FrameError.TOO_LARGE, tooLarge.reason());
        assertFalse(serialized.get());
        assertEquals(0, output.size());
    }
}

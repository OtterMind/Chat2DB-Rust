package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.ServerEnvelope;
import java.io.IOException;
import java.io.InterruptedIOException;
import java.io.OutputStream;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;

/** Dedicated single-owner writer for the binary stdout protocol stream. */
final class ProtocolWriter implements AutoCloseable {

    private static final int QUEUE_CAPACITY = 128;

    private final BlockingQueue<WriteRequest> queue = new ArrayBlockingQueue<>(QUEUE_CAPACITY);
    private final AtomicInteger peerMaximumFrameBytes =
            new AtomicInteger(FrameCodec.MAX_FRAME_BYTES);
    private final AtomicReference<IOException> failure = new AtomicReference<>();
    private final AtomicBoolean closing = new AtomicBoolean();
    private final Thread writerThread;

    ProtocolWriter(OutputStream output) {
        writerThread = new Thread(() -> runWriter(output), "chat2db-protocol-writer");
        writerThread.setDaemon(true);
        writerThread.start();
    }

    void setPeerMaximumFrameBytes(int maximum) {
        peerMaximumFrameBytes.set(Math.min(maximum, FrameCodec.MAX_FRAME_BYTES));
    }

    int peerMaximumFrameBytes() {
        return peerMaximumFrameBytes.get();
    }

    void write(ServerEnvelope envelope) throws IOException {
        if (closing.get()) {
            throw new IOException("protocol writer is closing");
        }
        checkHealth();
        int maximum = peerMaximumFrameBytes.get();
        int serializedSize = envelope.getSerializedSize();
        if (serializedSize == 0 || maximum <= 0 || serializedSize > maximum) {
            throw new FrameCodec.FrameException(
                    serializedSize == 0
                            ? FrameCodec.FrameError.EMPTY
                            : FrameCodec.FrameError.TOO_LARGE,
                    "response frame does not fit the negotiated peer limit");
        }
        WriteRequest request = WriteRequest.message(envelope);
        put(request);
        await(request.completion);
    }

    void checkHealth() throws IOException {
        IOException writerFailure = failure.get();
        if (writerFailure != null) {
            if (writerFailure instanceof FrameCodec.FrameException frameFailure) {
                throw new FrameCodec.FrameException(
                        frameFailure.reason(), frameFailure.getMessage(), frameFailure);
            }
            throw new IOException(writerFailure.getMessage(), writerFailure);
        }
    }

    @Override
    public void close() throws IOException {
        if (closing.compareAndSet(false, true) && writerThread.isAlive()) {
            WriteRequest poison = WriteRequest.poison();
            put(poison);
            await(poison.completion);
        }
        try {
            writerThread.join();
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            InterruptedIOException failure = new InterruptedIOException(
                    "interrupted while closing the protocol writer");
            failure.initCause(interrupted);
            throw failure;
        }
        checkHealth();
    }

    private void runWriter(OutputStream output) {
        while (true) {
            WriteRequest request;
            try {
                request = queue.take();
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                failWriter(new InterruptedIOException("protocol writer was interrupted"));
                return;
            }
            if (request.poison) {
                request.completion.complete(null);
                return;
            }
            try {
                FrameCodec.writeFrame(output, request.envelope, peerMaximumFrameBytes.get());
                request.completion.complete(null);
            } catch (IOException writerFailure) {
                request.completion.completeExceptionally(writerFailure);
                failWriter(writerFailure);
                return;
            } catch (RuntimeException writerFailure) {
                IOException wrapped = new IOException("protocol writer failed", writerFailure);
                request.completion.completeExceptionally(wrapped);
                failWriter(wrapped);
                return;
            }
        }
    }

    private void failWriter(IOException writerFailure) {
        failure.compareAndSet(null, writerFailure);
        WriteRequest queued;
        while ((queued = queue.poll()) != null) {
            queued.completion.completeExceptionally(writerFailure);
        }
    }

    private void put(WriteRequest request) throws IOException {
        while (true) {
            checkHealth();
            try {
                if (queue.offer(request, 100, TimeUnit.MILLISECONDS)) {
                    return;
                }
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                InterruptedIOException failure =
                        new InterruptedIOException("interrupted while queueing a protocol response");
                failure.initCause(interrupted);
                throw failure;
            }
        }
    }

    private void await(CompletableFuture<Void> completion) throws IOException {
        while (true) {
            try {
                completion.get(100, TimeUnit.MILLISECONDS);
                return;
            } catch (TimeoutException waiting) {
                checkHealth();
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                InterruptedIOException failure =
                        new InterruptedIOException("interrupted while writing a protocol response");
                failure.initCause(interrupted);
                throw failure;
            } catch (ExecutionException failed) {
                Throwable cause = failed.getCause();
                if (cause instanceof IOException ioFailure) {
                    throw ioFailure;
                }
                throw new IOException("protocol writer failed", cause);
            }
        }
    }

    private static final class WriteRequest {
        private final ServerEnvelope envelope;
        private final boolean poison;
        private final CompletableFuture<Void> completion = new CompletableFuture<>();

        private WriteRequest(ServerEnvelope envelope, boolean poison) {
            this.envelope = envelope;
            this.poison = poison;
        }

        private static WriteRequest message(ServerEnvelope envelope) {
            return new WriteRequest(envelope, false);
        }

        private static WriteRequest poison() {
            return new WriteRequest(null, true);
        }
    }
}

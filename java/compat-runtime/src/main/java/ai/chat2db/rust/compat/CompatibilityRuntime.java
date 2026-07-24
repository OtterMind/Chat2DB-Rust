package ai.chat2db.rust.compat;

import java.io.BufferedOutputStream;
import java.io.EOFException;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;

/** Process entrypoint for the private Java database compatibility engine. */
public final class CompatibilityRuntime {

    static final int EXIT_OK = 0;
    static final int EXIT_USAGE = 64;
    static final int EXIT_PROTOCOL = 65;
    static final int EXIT_INTERNAL = 70;
    static final int EXIT_IO = 74;
    static final int EXIT_INCOMPATIBLE = 78;

    private CompatibilityRuntime() {
    }

    public static void main(String[] args) {
        PrintStream diagnostics = System.err;
        OutputStream standardOutput = System.out;
        if (args.length == 0) {
            standardOutput = new BufferedOutputStream(new FileOutputStream(FileDescriptor.out));
            System.setOut(new PrintStream(
                    new BufferedOutputStream(new FileOutputStream(FileDescriptor.err)),
                    true,
                    StandardCharsets.UTF_8));
        }

        int exitCode = run(args, System.in, standardOutput, diagnostics);
        if (exitCode != 0) {
            System.exit(exitCode);
        }
    }

    static int run(
            String[] args,
            InputStream standardInput,
            OutputStream standardOutput,
            PrintStream standardError) {
        if (args.length == 1 && "--version".equals(args[0])) {
            try {
                standardOutput.write((RuntimeInfo.current().displayVersion() + System.lineSeparator())
                        .getBytes(StandardCharsets.UTF_8));
                standardOutput.flush();
                return EXIT_OK;
            } catch (IOException exception) {
                standardError.println("[compat-runtime] failed to write version output");
                return EXIT_IO;
            }
        }
        if (args.length != 0) {
            standardError.println("Usage: chat2db-java-compat [--version]");
            return EXIT_USAGE;
        }

        try {
            return new ProtocolLoop(standardError).serve(standardInput, standardOutput);
        } catch (FrameCodec.FrameException exception) {
            standardError.printf(
                    "[compat-runtime] invalid protocol frame reason=%s%n", exception.reason());
            return EXIT_PROTOCOL;
        } catch (EOFException exception) {
            standardError.println("[compat-runtime] truncated protocol frame");
            return EXIT_PROTOCOL;
        } catch (IOException exception) {
            standardError.println("[compat-runtime] protocol I/O failed");
            return EXIT_IO;
        } catch (RuntimeException exception) {
            standardError.println("[compat-runtime] internal protocol failure");
            return EXIT_INTERNAL;
        }
    }
}

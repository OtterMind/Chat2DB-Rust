package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import org.junit.jupiter.api.Test;

class CompatibilityRuntimeTest {

    @Test
    void explicitVersionModeRemainsOutsideTheBinaryProtocol() {
        ByteArrayOutputStream standardOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream standardError = new ByteArrayOutputStream();

        int exitCode = CompatibilityRuntime.run(
                new String[] {"--version"},
                new ByteArrayInputStream(new byte[0]),
                standardOutput,
                new PrintStream(standardError, true, StandardCharsets.UTF_8));

        assertEquals(CompatibilityRuntime.EXIT_OK, exitCode);
        assertEquals(
                "chat2db-java-compat development (protocol 1.0)" + System.lineSeparator(),
                standardOutput.toString(StandardCharsets.UTF_8));
        assertEquals("", standardError.toString(StandardCharsets.UTF_8));
    }

    @Test
    void cleanInputEofStopsWithoutWritingTextToProtocolStdout() {
        ByteArrayOutputStream standardOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream standardError = new ByteArrayOutputStream();

        int exitCode = CompatibilityRuntime.run(
                new String[0],
                new ByteArrayInputStream(new byte[0]),
                standardOutput,
                new PrintStream(standardError, true, StandardCharsets.UTF_8));

        assertEquals(CompatibilityRuntime.EXIT_OK, exitCode);
        assertEquals("", standardOutput.toString(StandardCharsets.UTF_8));
        assertTrue(standardError.toString(StandardCharsets.UTF_8).contains("stdin closed"));
    }

    @Test
    void invalidArgumentsFailWithoutContaminatingProtocolStdout() {
        ByteArrayOutputStream standardOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream standardError = new ByteArrayOutputStream();

        int exitCode = CompatibilityRuntime.run(
                new String[] {"--unknown"},
                new ByteArrayInputStream(new byte[0]),
                standardOutput,
                new PrintStream(standardError, true, StandardCharsets.UTF_8));

        assertEquals(CompatibilityRuntime.EXIT_USAGE, exitCode);
        assertEquals("", standardOutput.toString(StandardCharsets.UTF_8));
        assertTrue(standardError.toString(StandardCharsets.UTF_8).contains("Usage:"));
    }

    @Test
    void malformedFramesFailWithoutContaminatingProtocolStdout() {
        ByteArrayOutputStream standardOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream standardError = new ByteArrayOutputStream();

        int exitCode = CompatibilityRuntime.run(
                new String[0],
                new ByteArrayInputStream(new byte[] {0, 0, 0, 1, (byte) 0x80}),
                standardOutput,
                new PrintStream(standardError, true, StandardCharsets.UTF_8));

        assertEquals(CompatibilityRuntime.EXIT_PROTOCOL, exitCode);
        assertEquals("", standardOutput.toString(StandardCharsets.UTF_8));
        assertTrue(standardError.toString(StandardCharsets.UTF_8).contains("reason=MALFORMED"));
    }
}

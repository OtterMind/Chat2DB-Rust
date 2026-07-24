package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import org.junit.jupiter.api.Test;

class CompatibilityRuntimeTest {

    @Test
    void failsClosedBeforeTheProtocolIsEnabled() {
        ByteArrayOutputStream standardOutput = new ByteArrayOutputStream();
        ByteArrayOutputStream standardError = new ByteArrayOutputStream();

        int exitCode = CompatibilityRuntime.run(
                new String[0],
                new PrintStream(standardOutput, true, StandardCharsets.UTF_8),
                new PrintStream(standardError, true, StandardCharsets.UTF_8));

        assertEquals(2, exitCode);
        assertEquals("", standardOutput.toString(StandardCharsets.UTF_8));
        assertTrue(standardError.toString(StandardCharsets.UTF_8).contains("not enabled"));
    }
}

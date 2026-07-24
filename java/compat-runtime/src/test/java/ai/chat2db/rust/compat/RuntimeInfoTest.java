package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class RuntimeInfoTest {

    @Test
    void currentRuntimePublishesCompatibilityIdentity() {
        RuntimeInfo info = RuntimeInfo.current();

        assertEquals("chat2db-java-compat", info.name());
        assertEquals("development", info.version());
        assertEquals(0, info.protocolMajor());
        assertEquals("chat2db-java-compat development (protocol 0.1)", info.displayVersion());
    }

    @Test
    void rejectsNegativeProtocolVersions() {
        assertThrows(
                IllegalArgumentException.class,
                () -> new RuntimeInfo("runtime", "version", -1, 0));
    }
}

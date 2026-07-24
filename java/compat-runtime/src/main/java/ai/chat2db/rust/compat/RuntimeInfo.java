package ai.chat2db.rust.compat;

import java.util.Objects;
import java.util.Optional;

/** Stable identity of the private Java compatibility process. */
public record RuntimeInfo(String name, String version, int protocolMajor, int protocolMinor) {

    public RuntimeInfo {
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(version, "version");
        if (protocolMajor < 0 || protocolMinor < 0) {
            throw new IllegalArgumentException("protocol versions must be non-negative");
        }
    }

    public static RuntimeInfo current() {
        String implementationVersion = Optional.ofNullable(
                        RuntimeInfo.class.getPackage().getImplementationVersion())
                .orElse("development");
        return new RuntimeInfo(
                "chat2db-java-compat",
                implementationVersion,
                ProtocolLoop.PROTOCOL_MAJOR,
                ProtocolLoop.PROTOCOL_MINOR);
    }

    public String displayVersion() {
        return "%s %s (protocol %d.%d)".formatted(name, version, protocolMajor, protocolMinor);
    }
}

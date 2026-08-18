package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.ConnectionProperty;
import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.OpenSessionRequest;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.Set;
import java.util.UUID;

/** Owns live JDBC sessions and their corresponding driver leases. */
final class SessionRegistry implements AutoCloseable {

    private final DriverRegistry drivers;
    private final Map<String, JdbcSession> sessions = new HashMap<>();
    private boolean closed;

    SessionRegistry(DriverRegistry drivers) {
        this.drivers = drivers;
    }

    JdbcSession open(OpenSessionRequest request) throws RuntimeFailure {
        synchronized (this) {
            ensureOpen();
        }
        ProtocolLimits.requireNonBlankUtf8(
                request.getDriverId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "driver_id");
        ProtocolLimits.requireNonBlankUtf8(
                request.getJdbcUrl(), ProtocolLimits.MAX_JDBC_URL_BYTES, "jdbc_url");
        ConnectionSettings settings = validateProperties(request.getPropertiesList());

        DriverRegistry.DriverLease lease = drivers.acquire(request.getDriverId());
        Connection connection = null;
        try {
            connection = lease.connect(request.getJdbcUrl(), settings.properties());
            if (connection == null) {
                throw RuntimeFailure.validation(
                        "driver.url_not_accepted", "the selected driver does not accept the JDBC URL");
            }
            if (!connection.getAutoCommit()) {
                connection.setAutoCommit(true);
            }
            connection.setReadOnly(request.getReadOnly());
            boolean readOnly = connection.isReadOnly();
            int isolation = connection.getTransactionIsolation();
            DatabaseMetaData metadata = connection.getMetaData();
            DatabaseProduct database = DatabaseProduct.newBuilder()
                    .setName(scalar(metadata.getDatabaseProductName()))
                    .setVersion(scalar(metadata.getDatabaseProductVersion()))
                    .setDriverName(scalar(metadata.getDriverName()))
                    .setDriverVersion(scalar(metadata.getDriverVersion()))
                    .build();
            String sessionId = UUID.randomUUID().toString();
            JdbcSession session =
                    new JdbcSession(
                            sessionId,
                            connection,
                            lease,
                            database,
                            readOnly,
                            isolation,
                            settings.redactor());
            synchronized (this) {
                ensureOpen();
                sessions.put(sessionId, session);
            }
            connection = null;
            lease = null;
            return session;
        } catch (RuntimeFailure failure) {
            throw failure.withRedactor(settings.redactor());
        } catch (SQLException failure) {
            throw RuntimeFailure.database(
                    "session.open_failed",
                    "the JDBC session could not be opened",
                    failure,
                    ai.chat2db.rust.compat.protocol.v1.OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                    false).withRedactor(settings.redactor());
        } catch (RuntimeException failure) {
            throw RuntimeFailure.internal(
                    "session.open_internal_failure",
                    "the JDBC session could not be opened",
                    failure).withRedactor(settings.redactor());
        } finally {
            cleanupFailedOpen(connection, lease);
        }
    }

    static void cleanupFailedOpen(
            Connection connection, DriverRegistry.DriverLease lease) {
        try {
            if (connection != null) {
                connection.close();
            }
        } catch (SQLException | RuntimeException ignored) {
            // The original open failure remains authoritative.
        } finally {
            if (lease != null) {
                lease.close();
            }
        }
    }

    synchronized JdbcSession require(String sessionId) throws RuntimeFailure {
        ensureOpen();
        ProtocolLimits.requireNonBlankUtf8(
                sessionId, ProtocolLimits.MAX_DRIVER_ID_BYTES, "session_id");
        JdbcSession session = sessions.get(sessionId);
        if (session == null) {
            throw RuntimeFailure.validation(
                    "session.not_found", "the requested session_id does not exist");
        }
        return session;
    }

    synchronized int activeCount() {
        return sessions.size();
    }

    void close(String sessionId) throws RuntimeFailure {
        JdbcSession session = require(sessionId);
        try {
            session.close();
        } catch (RuntimeFailure failure) {
            if (session.state() == JdbcSession.State.CLOSED) {
                synchronized (this) {
                    sessions.remove(sessionId, session);
                }
            }
            throw failure;
        }
        synchronized (this) {
            sessions.remove(sessionId, session);
        }
    }

    @Override
    public void close() {
        closeAll();
    }

    boolean closeAll() {
        List<JdbcSession> snapshot;
        synchronized (this) {
            if (closed && sessions.isEmpty()) {
                return true;
            }
            closed = true;
            snapshot = new ArrayList<>(sessions.values());
        }
        for (JdbcSession session : snapshot) {
            try {
                session.close();
            } catch (RuntimeFailure ignored) {
                // Ownership remains in the registry for process-exit cleanup.
            }
            if (session.state() == JdbcSession.State.CLOSED) {
                synchronized (this) {
                    sessions.remove(session.id(), session);
                }
            }
        }
        synchronized (this) {
            return sessions.isEmpty();
        }
    }

    private void ensureOpen() throws RuntimeFailure {
        if (closed) {
            throw RuntimeFailure.conflict("session.registry_closed", "the session registry is closed");
        }
    }

    private static ConnectionSettings validateProperties(List<ConnectionProperty> requested)
            throws RuntimeFailure {
        if (requested.size() > ProtocolLimits.MAX_CONNECTION_PROPERTIES) {
            throw RuntimeFailure.limit(
                    "connection_properties", ProtocolLimits.MAX_CONNECTION_PROPERTIES);
        }
        Properties properties = new Properties();
        Set<String> keys = new HashSet<>();
        List<String> sensitiveValues = new ArrayList<>();
        for (ConnectionProperty property : requested) {
            ProtocolLimits.requireNonBlankUtf8(
                    property.getKey(), ProtocolLimits.MAX_PROPERTY_KEY_BYTES, "property_key");
            ProtocolLimits.requireUtf8(
                    property.getValue(), ProtocolLimits.MAX_PROPERTY_VALUE_BYTES, "property_value");
            if (!keys.add(property.getKey())) {
                throw RuntimeFailure.validation(
                        "session.duplicate_property", "connection property keys must be unique");
            }
            properties.setProperty(property.getKey(), property.getValue());
            if (property.getSensitive()) {
                sensitiveValues.add(property.getValue());
            }
        }
        return new ConnectionSettings(
                properties, new SensitiveDataRedactor(sensitiveValues));
    }

    private static String scalar(String value) {
        return ProtocolLimits.truncateUtf8(value == null ? "" : value, ProtocolLimits.MAX_SCALAR_BYTES);
    }

    private record ConnectionSettings(
            Properties properties, SensitiveDataRedactor redactor) {
    }
}

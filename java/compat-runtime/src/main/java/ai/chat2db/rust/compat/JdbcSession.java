package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import ai.chat2db.rust.compat.protocol.v1.TransactionIsolation;
import java.sql.Connection;
import java.sql.SQLException;
import java.util.Objects;
import java.util.Optional;
import java.util.UUID;

/** A single-owner JDBC connection with an explicit local transaction state machine. */
final class JdbcSession implements AutoCloseable {

    enum State {
        READY,
        IN_TRANSACTION,
        ROLLBACK_REQUIRED,
        BROKEN,
        CLOSED
    }

    private final String id;
    private final Connection connection;
    private final DriverRegistry.DriverLease driverLease;
    private final DatabaseProduct database;
    private final boolean defaultReadOnly;
    private final int defaultIsolation;
    private final SensitiveDataRedactor redactor;

    private State state = State.READY;
    private String transactionId;
    private String activeOperationId;

    JdbcSession(
            String id,
            Connection connection,
            DriverRegistry.DriverLease driverLease,
            DatabaseProduct database,
            boolean defaultReadOnly,
            int defaultIsolation,
            SensitiveDataRedactor redactor) {
        this.id = id;
        this.connection = connection;
        this.driverLease = driverLease;
        this.database = database;
        this.defaultReadOnly = defaultReadOnly;
        this.defaultIsolation = defaultIsolation;
        this.redactor = redactor;
    }

    String id() {
        return id;
    }

    DatabaseProduct database() {
        return database;
    }

    boolean defaultReadOnly() {
        return defaultReadOnly;
    }

    synchronized State state() {
        return state;
    }

    synchronized SessionState protocolState() {
        return switch (state) {
            case READY -> SessionState.SESSION_STATE_AUTO_COMMIT;
            case IN_TRANSACTION -> SessionState.SESSION_STATE_TRANSACTION_ACTIVE;
            case ROLLBACK_REQUIRED -> SessionState.SESSION_STATE_ROLLBACK_REQUIRED;
            case BROKEN -> SessionState.SESSION_STATE_BROKEN;
            case CLOSED -> SessionState.SESSION_STATE_CLOSED;
        };
    }

    synchronized RuntimeFailure decorate(RuntimeFailure failure) {
        return failure.withSessionState(protocolState()).withRedactor(redactor);
    }

    synchronized Optional<String> activeOperationId() {
        return Optional.ofNullable(activeOperationId);
    }

    synchronized Connection claimOperation(String requestId, Optional<String> requestedTransactionId)
            throws RuntimeFailure {
        ensureUsable();
        if (state == State.ROLLBACK_REQUIRED) {
            throw RuntimeFailure.conflict(
                    "transaction.rollback_required",
                    "the session requires rollback before another operation");
        }
        if (activeOperationId != null) {
            throw RuntimeFailure.conflict(
                    "session.operation_in_progress", "the session already has an active operation");
        }
        validateTransactionBinding(requestedTransactionId);
        activeOperationId = requestId;
        return connection;
    }

    synchronized void finishOperation(String requestId) {
        if (Objects.equals(activeOperationId, requestId)) {
            activeOperationId = null;
        }
    }

    synchronized void markQueryFailure() {
        if (state == State.IN_TRANSACTION) {
            state = State.ROLLBACK_REQUIRED;
        }
    }

    synchronized void markUpdateFailure() {
        if (state == State.IN_TRANSACTION) {
            state = State.ROLLBACK_REQUIRED;
        } else if (state == State.READY) {
            state = State.BROKEN;
        }
    }

    synchronized void markBroken() {
        if (state != State.CLOSED) {
            state = State.BROKEN;
        }
    }

    TransactionDescriptor begin(TransactionIsolation requested, boolean readOnly)
            throws RuntimeFailure {
        int isolation = toJdbcIsolation(requested);
        String controlId = "transaction-begin:" + UUID.randomUUID();
        synchronized (this) {
            ensureUsable();
            ensureIdle();
            if (state != State.READY) {
                throw RuntimeFailure.conflict(
                        "transaction.already_active", "the session already has a local transaction");
            }
            activeOperationId = controlId;
        }

        try {
            if (isolation != 0) {
                connection.setTransactionIsolation(isolation);
            }
            connection.setReadOnly(readOnly);
            connection.setAutoCommit(false);
            int effectiveIsolation = connection.getTransactionIsolation();
            boolean effectiveReadOnly = connection.isReadOnly();
            synchronized (this) {
                transactionId = UUID.randomUUID().toString();
                state = State.IN_TRANSACTION;
                return new TransactionDescriptor(
                        transactionId, fromJdbcIsolation(effectiveIsolation), effectiveReadOnly);
            }
        } catch (SQLException failure) {
            bestEffortRollbackAndRestore();
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.database(
                    "transaction.begin_failed",
                    "the local transaction could not be started",
                    failure,
                    OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                    false));
        } catch (RuntimeException failure) {
            bestEffortRollbackAndRestore();
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.internal(
                            "transaction.begin_outcome_unknown",
                            "the transaction begin outcome is unknown and the session is broken",
                            failure)
                    .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
        } finally {
            finishOperation(controlId);
        }
    }

    String commit(String requestedTransactionId) throws RuntimeFailure {
        String controlId = "transaction-commit:" + UUID.randomUUID();
        String committed;
        synchronized (this) {
            ensureUsable();
            ensureIdle();
            if (state == State.ROLLBACK_REQUIRED) {
                throw RuntimeFailure.conflict(
                        "transaction.rollback_required",
                        "the failed transaction must be rolled back instead of committed");
            }
            requireTransaction(requestedTransactionId);
            committed = transactionId;
            activeOperationId = controlId;
        }
        try {
            connection.commit();
            restoreConnectionDefaults();
            synchronized (this) {
                transactionId = null;
                state = State.READY;
            }
            return committed;
        } catch (SQLException failure) {
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.database(
                    "transaction.commit_outcome_unknown",
                    "the transaction commit outcome is unknown and the session is broken",
                    failure,
                    OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                    false));
        } catch (RuntimeException failure) {
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.internal(
                            "transaction.commit_outcome_unknown",
                            "the transaction commit outcome is unknown and the session is broken",
                            failure)
                    .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
        } finally {
            finishOperation(controlId);
        }
    }

    String rollback(String requestedTransactionId) throws RuntimeFailure {
        String controlId = "transaction-rollback:" + UUID.randomUUID();
        String rolledBack;
        synchronized (this) {
            if (state == State.CLOSED) {
                throw RuntimeFailure.conflict("session.closed", "the JDBC session is closed");
            }
            if (state == State.BROKEN) {
                throw RuntimeFailure.conflict(
                        "session.broken", "the JDBC session is broken and must be closed");
            }
            ensureIdle();
            requireTransaction(requestedTransactionId);
            rolledBack = transactionId;
            activeOperationId = controlId;
        }
        try {
            connection.rollback();
            restoreConnectionDefaults();
            synchronized (this) {
                transactionId = null;
                state = State.READY;
            }
            return rolledBack;
        } catch (SQLException failure) {
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.database(
                    "transaction.rollback_outcome_unknown",
                    "the transaction rollback outcome is unknown and the session is broken",
                    failure,
                    OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                    false));
        } catch (RuntimeException failure) {
            synchronized (this) {
                state = State.BROKEN;
            }
            throw decorate(RuntimeFailure.internal(
                            "transaction.rollback_outcome_unknown",
                            "the transaction rollback outcome is unknown and the session is broken",
                            failure)
                    .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN));
        } finally {
            finishOperation(controlId);
        }
    }

    @Override
    public void close() throws RuntimeFailure {
        synchronized (this) {
            if (state == State.CLOSED) {
                return;
            }
            ensureIdle();
            activeOperationId = "session-close";
        }
        Throwable closeFailure = null;
        boolean connectionClosed = false;
        try {
            try {
                if (!connection.getAutoCommit()) {
                    connection.rollback();
                }
            } catch (SQLException | RuntimeException failure) {
                closeFailure = append(closeFailure, failure);
            }
            try {
                connection.close();
                connectionClosed = true;
            } catch (SQLException | RuntimeException failure) {
                closeFailure = append(closeFailure, failure);
                try {
                    connectionClosed = connection.isClosed();
                } catch (SQLException | RuntimeException confirmationFailure) {
                    closeFailure = append(closeFailure, confirmationFailure);
                    connectionClosed = false;
                }
            }
        } finally {
            boolean releaseLease;
            synchronized (this) {
                activeOperationId = null;
                if (connectionClosed) {
                    state = State.CLOSED;
                    transactionId = null;
                    releaseLease = true;
                } else {
                    state = State.BROKEN;
                    releaseLease = false;
                }
            }
            if (releaseLease) {
                driverLease.close();
            }
        }
        if (closeFailure != null) {
            RuntimeFailure failure = closeFailure instanceof SQLException sqlFailure
                    ? RuntimeFailure.database(
                            "session.close_failed",
                            "the JDBC session could not be closed cleanly",
                            sqlFailure,
                            OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                            false)
                    : RuntimeFailure.internal(
                                    "session.close_failed",
                                    "the JDBC session could not be closed cleanly",
                                    closeFailure)
                            .withOutcome(OperationOutcome.OPERATION_OUTCOME_UNKNOWN);
            throw decorate(failure);
        }
    }

    private void validateTransactionBinding(Optional<String> requestedTransactionId)
            throws RuntimeFailure {
        if (requestedTransactionId.isPresent()) {
            ProtocolLimits.requireNonBlankUtf8(
                    requestedTransactionId.orElseThrow(),
                    ProtocolLimits.MAX_DRIVER_ID_BYTES,
                    "transaction_id");
        }
        if (state == State.IN_TRANSACTION) {
            if (requestedTransactionId.isEmpty()
                    || !Objects.equals(transactionId, requestedTransactionId.orElseThrow())) {
                throw RuntimeFailure.validation(
                        "transaction.id_mismatch",
                        "the active transaction_id is required for this operation");
            }
        } else if (requestedTransactionId.isPresent()) {
            throw RuntimeFailure.validation(
                    "transaction.not_active", "the session does not have an active transaction");
        }
    }

    private void requireTransaction(String requestedTransactionId) throws RuntimeFailure {
        ProtocolLimits.requireNonBlankUtf8(
                requestedTransactionId,
                ProtocolLimits.MAX_DRIVER_ID_BYTES,
                "transaction_id");
        if (state != State.IN_TRANSACTION && state != State.ROLLBACK_REQUIRED) {
            throw RuntimeFailure.validation(
                    "transaction.not_active", "the session does not have an active transaction");
        }
        if (requestedTransactionId == null
                || requestedTransactionId.isBlank()
                || !Objects.equals(transactionId, requestedTransactionId)) {
            throw RuntimeFailure.validation(
                    "transaction.id_mismatch", "transaction_id does not match the active transaction");
        }
    }

    private void ensureUsable() throws RuntimeFailure {
        if (state == State.CLOSED) {
            throw RuntimeFailure.conflict("session.closed", "the JDBC session is closed");
        }
        if (state == State.BROKEN) {
            throw RuntimeFailure.conflict(
                    "session.broken", "the JDBC session is broken and must be closed");
        }
    }

    private void ensureIdle() throws RuntimeFailure {
        if (activeOperationId != null) {
            throw RuntimeFailure.conflict(
                    "session.operation_in_progress", "the session already has an active operation");
        }
    }

    private void restoreConnectionDefaults() throws SQLException {
        connection.setAutoCommit(true);
        connection.setReadOnly(defaultReadOnly);
        connection.setTransactionIsolation(defaultIsolation);
    }

    private void bestEffortRollbackAndRestore() {
        try {
            if (!connection.getAutoCommit()) {
                connection.rollback();
            }
        } catch (SQLException | RuntimeException ignored) {
            // State is marked broken below.
        }
        try {
            restoreConnectionDefaults();
        } catch (SQLException | RuntimeException ignored) {
            // State is marked broken below.
        }
    }

    private static Throwable append(Throwable current, Throwable additional) {
        if (current == null) {
            return additional;
        }
        if (current != additional) {
            current.addSuppressed(additional);
        }
        return current;
    }

    private static int toJdbcIsolation(TransactionIsolation isolation) throws RuntimeFailure {
        return switch (isolation) {
            case TRANSACTION_ISOLATION_DEFAULT -> 0;
            case TRANSACTION_ISOLATION_READ_UNCOMMITTED -> Connection.TRANSACTION_READ_UNCOMMITTED;
            case TRANSACTION_ISOLATION_READ_COMMITTED -> Connection.TRANSACTION_READ_COMMITTED;
            case TRANSACTION_ISOLATION_REPEATABLE_READ -> Connection.TRANSACTION_REPEATABLE_READ;
            case TRANSACTION_ISOLATION_SERIALIZABLE -> Connection.TRANSACTION_SERIALIZABLE;
            case UNRECOGNIZED -> throw RuntimeFailure.validation(
                    "transaction.invalid_isolation", "transaction isolation is not recognized");
        };
    }

    private static TransactionIsolation fromJdbcIsolation(int isolation) {
        return switch (isolation) {
            case Connection.TRANSACTION_READ_UNCOMMITTED ->
                    TransactionIsolation.TRANSACTION_ISOLATION_READ_UNCOMMITTED;
            case Connection.TRANSACTION_READ_COMMITTED ->
                    TransactionIsolation.TRANSACTION_ISOLATION_READ_COMMITTED;
            case Connection.TRANSACTION_REPEATABLE_READ ->
                    TransactionIsolation.TRANSACTION_ISOLATION_REPEATABLE_READ;
            case Connection.TRANSACTION_SERIALIZABLE ->
                    TransactionIsolation.TRANSACTION_ISOLATION_SERIALIZABLE;
            default -> TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT;
        };
    }

    record TransactionDescriptor(
            String transactionId, TransactionIsolation isolation, boolean readOnly) {
    }
}

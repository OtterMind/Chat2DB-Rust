package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.DatabaseErrorCause;
import ai.chat2db.rust.compat.protocol.v1.DatabaseErrorDetail;
import ai.chat2db.rust.compat.protocol.v1.EngineError;
import ai.chat2db.rust.compat.protocol.v1.ErrorCategory;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import java.sql.SQLException;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.Set;

/** Structured runtime failure that is safe to translate to the wire protocol. */
final class RuntimeFailure extends Exception {

    private static final int MAX_ERROR_MESSAGE_BYTES = 4096;
    private static final int MAX_CAUSE_MESSAGE_BYTES = 2048;
    private static final int MAX_CLASS_NAME_BYTES = 512;
    private static final int MAX_SQL_STATE_BYTES = 128;

    private final String code;
    private final ErrorCategory category;
    private final OperationOutcome outcome;
    private final boolean retryable;
    private final SQLException databaseCause;
    private SessionState sessionState;
    private SensitiveDataRedactor redactor = SensitiveDataRedactor.NONE;

    private RuntimeFailure(
            String code,
            String message,
            ErrorCategory category,
            OperationOutcome outcome,
            boolean retryable,
            Throwable cause,
            SQLException databaseCause) {
        super(message, cause);
        this.code = code;
        this.category = category;
        this.outcome = outcome;
        this.retryable = retryable;
        this.databaseCause = databaseCause;
    }

    static RuntimeFailure validation(String code, String message) {
        return new RuntimeFailure(
                code,
                message,
                ErrorCategory.ERROR_CATEGORY_VALIDATION,
                OperationOutcome.OPERATION_OUTCOME_NOT_STARTED,
                false,
                null,
                null);
    }

    static RuntimeFailure conflict(String code, String message) {
        return new RuntimeFailure(
                code,
                message,
                ErrorCategory.ERROR_CATEGORY_UNAVAILABLE,
                OperationOutcome.OPERATION_OUTCOME_NOT_STARTED,
                true,
                null,
                null);
    }

    static RuntimeFailure internal(String code, String message, Throwable cause) {
        return new RuntimeFailure(
                code,
                message,
                ErrorCategory.ERROR_CATEGORY_INTERNAL,
                OperationOutcome.OPERATION_OUTCOME_NOT_STARTED,
                false,
                cause,
                null);
    }

    static RuntimeFailure database(
            String code,
            String message,
            SQLException cause,
            OperationOutcome outcome,
            boolean retryable) {
        return new RuntimeFailure(
                code,
                message,
                ErrorCategory.ERROR_CATEGORY_DATABASE,
                outcome,
                retryable,
                cause,
                cause);
    }

    static RuntimeFailure cancelled(String message) {
        return new RuntimeFailure(
                "database.operation_cancelled",
                message,
                ErrorCategory.ERROR_CATEGORY_CANCELLED,
                OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                false,
                null,
                null);
    }

    static RuntimeFailure deadline(String message) {
        return new RuntimeFailure(
                "database.deadline_exceeded",
                message,
                ErrorCategory.ERROR_CATEGORY_DEADLINE,
                OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                true,
                null,
                null);
    }

    static RuntimeFailure limit(String field, int maximum) {
        return validation(
                "protocol.limit_exceeded",
                field + " exceeds the hard limit of " + maximum + " bytes or items");
    }

    String code() {
        return code;
    }

    OperationOutcome outcome() {
        return outcome;
    }

    RuntimeFailure withSessionState(SessionState state) {
        this.sessionState = state;
        return this;
    }

    RuntimeFailure withRedactor(SensitiveDataRedactor replacement) {
        redactor = replacement == null ? SensitiveDataRedactor.NONE : replacement;
        return this;
    }

    RuntimeFailure withOutcome(OperationOutcome replacement) {
        RuntimeFailure copy = new RuntimeFailure(
                code,
                getMessage(),
                category,
                replacement,
                retryable,
                getCause(),
                databaseCause);
        copy.sessionState = sessionState;
        copy.redactor = redactor;
        return copy;
    }

    EngineError toEngineError() {
        EngineError.Builder error = EngineError.newBuilder()
                .setCode(code)
                .setMessage(redactor.redactAndTruncate(
                        getMessage() == null ? "" : getMessage(), MAX_ERROR_MESSAGE_BYTES))
                .setCategory(category)
                .setRetryable(retryable)
                .setFatal(false)
                .setOutcome(outcome);
        if (sessionState != null) {
            error.setSessionState(sessionState);
        }
        if (databaseCause != null) {
            error.setDatabaseError(databaseDetail(databaseCause, redactor));
        }
        return error.build();
    }

    private static DatabaseErrorDetail databaseDetail(
            SQLException failure, SensitiveDataRedactor redactor) {
        DatabaseErrorDetail.Builder detail = DatabaseErrorDetail.newBuilder();
        if (failure.getSQLState() != null) {
            detail.setSqlState(
                    redactor.redactAndTruncate(failure.getSQLState(), MAX_SQL_STATE_BYTES));
        }
        detail.setVendorCode(failure.getErrorCode());

        Set<Throwable> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        Throwable current = failure;
        int count = 0;
        while (current != null && seen.add(current) && count < ProtocolLimits.MAX_ERROR_CAUSES) {
            DatabaseErrorCause.Builder cause = DatabaseErrorCause.newBuilder()
                    .setClassName(truncate(current.getClass().getName(), MAX_CLASS_NAME_BYTES))
                    .setMessage(redactor.redactAndTruncate(
                            current.getMessage() == null ? "" : current.getMessage(),
                            MAX_CAUSE_MESSAGE_BYTES));
            if (current instanceof SQLException sqlException) {
                if (sqlException.getSQLState() != null) {
                    cause.setSqlState(redactor.redactAndTruncate(
                            sqlException.getSQLState(), MAX_SQL_STATE_BYTES));
                }
                cause.setVendorCode(sqlException.getErrorCode());
                current = sqlException.getNextException() != null
                        ? sqlException.getNextException()
                        : sqlException.getCause();
            } else {
                current = current.getCause();
            }
            detail.addCauses(cause);
            count++;
        }
        return detail.build();
    }

    private static String truncate(String value, int maximumBytes) {
        return ProtocolLimits.truncateUtf8(value == null ? "" : value, maximumBytes);
    }
}

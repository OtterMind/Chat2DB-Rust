package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.CancelDisposition;
import ai.chat2db.rust.compat.protocol.v1.RequestMeta;
import java.sql.Connection;
import java.sql.SQLException;
import java.sql.Statement;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Future;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.ScheduledThreadPoolExecutor;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

/** Tracks query workers, bounded credits, cancellation, and retired request ids. */
final class OperationRegistry implements AutoCloseable {

    private static final int MAX_RETIRED_REQUESTS = 4096;

    private final Map<String, QueryOperation> active = new ConcurrentHashMap<>();
    private final Set<String> retired = new LinkedHashSet<>();
    private final ThreadPoolExecutor workers;
    private final ThreadPoolExecutor cancellers;
    private final ScheduledThreadPoolExecutor watchdogs;
    private volatile boolean closed;

    OperationRegistry() {
        int workerCount = Math.max(2, Math.min(32, Runtime.getRuntime().availableProcessors()));
        AtomicInteger threadNumber = new AtomicInteger();
        ThreadFactory factory = task -> {
            Thread thread = new Thread(task, "chat2db-jdbc-query-" + threadNumber.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        };
        workers = new ThreadPoolExecutor(
                workerCount,
                workerCount,
                0,
                TimeUnit.MILLISECONDS,
                new ArrayBlockingQueue<>(256),
                factory,
                new ThreadPoolExecutor.AbortPolicy());
        AtomicInteger cancelThreadNumber = new AtomicInteger();
        ThreadFactory cancelFactory = task -> {
            Thread thread = new Thread(
                    task, "chat2db-jdbc-cancel-" + cancelThreadNumber.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        };
        cancellers = new ThreadPoolExecutor(
                2,
                2,
                0,
                TimeUnit.MILLISECONDS,
                new ArrayBlockingQueue<>(256),
                cancelFactory,
                new ThreadPoolExecutor.AbortPolicy());
        ThreadFactory watchdogFactory = task -> {
            Thread thread = new Thread(task, "chat2db-jdbc-deadline-watchdog");
            thread.setDaemon(true);
            return thread;
        };
        watchdogs = new ScheduledThreadPoolExecutor(1, watchdogFactory);
        watchdogs.setRemoveOnCancelPolicy(true);
    }

    QueryOperation register(
            JdbcSession session,
            RequestMeta meta,
            int initialCredits,
            Optional<String> transactionId)
            throws RuntimeFailure {
        if (closed) {
            throw RuntimeFailure.conflict(
                    "operation.registry_closed", "the operation registry is closed");
        }
        ProtocolLimits.requireNonBlankUtf8(
                meta.getRequestId(), ProtocolLimits.MAX_DRIVER_ID_BYTES, "request_id");
        if (initialCredits < 0 || initialCredits > ProtocolLimits.MAX_CREDIT_GRANT) {
            throw RuntimeFailure.limit(
                    "initial_batch_credits", ProtocolLimits.MAX_CREDIT_GRANT);
        }
        if (meta.hasDeadlineUnixMillis()
                && meta.getDeadlineUnixMillis() <= System.currentTimeMillis()) {
            throw RuntimeFailure.deadline("the operation deadline elapsed before registration");
        }
        synchronized (retired) {
            if (retired.contains(meta.getRequestId()) || active.containsKey(meta.getRequestId())) {
                throw RuntimeFailure.validation(
                        "operation.duplicate_request_id",
                        "request_id must be unique for the lifetime of the engine process");
            }
        }

        Connection connection = session.claimOperation(meta.getRequestId(), transactionId);
        QueryOperation operation = new QueryOperation(
                session,
                connection,
                meta,
                initialCredits,
                meta.hasDeadlineUnixMillis() ? meta.getDeadlineUnixMillis() : 0);
        if (active.putIfAbsent(meta.getRequestId(), operation) != null) {
            session.finishOperation(meta.getRequestId());
            throw RuntimeFailure.validation(
                    "operation.duplicate_request_id",
                    "request_id must be unique for the lifetime of the engine process");
        }
        if (operation.deadlineUnixMillis() > 0) {
            long delayMillis = Math.max(
                    0, operation.deadlineUnixMillis() - System.currentTimeMillis());
            try {
                ScheduledFuture<?> watchdog = watchdogs.schedule(
                        () -> requestCancellation(operation, CancellationReason.DEADLINE),
                        delayMillis,
                        TimeUnit.MILLISECONDS);
                operation.setDeadlineWatchdog(watchdog);
            } catch (RuntimeException rejected) {
                complete(operation);
                throw RuntimeFailure.conflict(
                        "operation.watchdog_unavailable",
                        "the deadline watchdog is not available");
            }
        }
        return operation;
    }

    void submit(QueryOperation operation, Runnable task) throws RuntimeFailure {
        try {
            Future<?> future = workers.submit(() -> {
                try {
                    task.run();
                } finally {
                    complete(operation);
                }
            });
            operation.setFuture(future);
        } catch (RuntimeException failure) {
            complete(operation);
            throw RuntimeFailure.conflict(
                    "operation.worker_unavailable", "no query worker is currently available");
        }
    }

    int grantCredits(String requestId, int credits) throws RuntimeFailure {
        if (credits <= 0 || credits > ProtocolLimits.MAX_CREDIT_GRANT) {
            throw RuntimeFailure.validation(
                    "operation.invalid_credit_grant",
                    "batch_credits must be between 1 and " + ProtocolLimits.MAX_CREDIT_GRANT);
        }
        QueryOperation operation = active.get(requestId);
        if (operation == null) {
            throw RuntimeFailure.validation(
                    "operation.not_active", "target_request_id does not identify an active query");
        }
        operation.addCredits(credits);
        return credits;
    }

    CancelDisposition cancel(String requestId) {
        QueryOperation operation = active.get(requestId);
        if (operation != null) {
            CancellationRequest cancellation =
                    requestCancellation(operation, CancellationReason.USER);
            if (!cancellation.accepted()) {
                return CancelDisposition.CANCEL_DISPOSITION_ALREADY_TERMINAL;
            }
            return CancelDisposition.CANCEL_DISPOSITION_ACCEPTED;
        }
        synchronized (retired) {
            return retired.contains(requestId)
                    ? CancelDisposition.CANCEL_DISPOSITION_ALREADY_TERMINAL
                    : CancelDisposition.CANCEL_DISPOSITION_UNKNOWN_REQUEST;
        }
    }

    boolean cancelAllAndAwait(Duration timeout) {
        List<QueryOperation> snapshot = new ArrayList<>(active.values());
        snapshot.forEach(operation -> cancel(operation.requestId()));
        long deadlineNanos = System.nanoTime() + Math.max(0, timeout.toNanos());
        while (!active.isEmpty() && System.nanoTime() < deadlineNanos) {
            try {
                TimeUnit.MILLISECONDS.sleep(10);
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                return false;
            }
        }
        return active.isEmpty();
    }

    void finish(QueryOperation operation) {
        complete(operation);
    }

    void deferFinish(QueryOperation operation, Runnable cleanup) {
        operation.deferOwnershipRelease(cleanup, () -> completeNow(operation));
    }

    @Override
    public void close() {
        close(Duration.ofSeconds(5));
    }

    boolean close(Duration timeout) {
        closed = true;
        long deadlineNanos = System.nanoTime() + Math.max(0, timeout.toNanos());
        List<QueryOperation> snapshot = new ArrayList<>(active.values());
        snapshot.forEach(operation -> requestCancellation(operation, CancellationReason.USER));
        watchdogs.shutdownNow();
        workers.shutdown();
        cancellers.shutdown();
        boolean interrupted = false;
        try {
            awaitTermination(workers, deadlineNanos);
            awaitTermination(cancellers, deadlineNanos);
            awaitTermination(watchdogs, deadlineNanos);
        } catch (InterruptedException interruption) {
            Thread.currentThread().interrupt();
            interrupted = true;
        }
        if (!workers.isTerminated()) {
            workers.shutdownNow();
        }
        if (!cancellers.isTerminated()) {
            cancellers.shutdownNow();
        }
        return !interrupted
                && active.isEmpty()
                && workers.isTerminated()
                && cancellers.isTerminated()
                && watchdogs.isTerminated();
    }

    private CancellationRequest requestCancellation(
            QueryOperation operation, CancellationReason reason) {
        CancellationRequest request = operation.requestCancellation(reason);
        if (!request.accepted() || request.statement() == null) {
            return request;
        }
        try {
            cancellers.execute(() -> operation.cancelInstalledStatement(request.statement()));
        } catch (RuntimeException rejected) {
            operation.recordCancellationFailure(
                    new SQLException("the cancellation worker queue is unavailable"));
        }
        return request;
    }

    private static void awaitTermination(
            java.util.concurrent.ExecutorService executor, long deadlineNanos)
            throws InterruptedException {
        long remaining = deadlineNanos - System.nanoTime();
        if (remaining > 0) {
            executor.awaitTermination(remaining, TimeUnit.NANOSECONDS);
        }
    }

    private void complete(QueryOperation operation) {
        if (operation.completionDeferred()) {
            return;
        }
        completeNow(operation);
    }

    private void completeNow(QueryOperation operation) {
        operation.markTerminal();
        if (!active.remove(operation.requestId(), operation)) {
            return;
        }
        operation.session().finishOperation(operation.requestId());
        synchronized (retired) {
            retired.add(operation.requestId());
            while (retired.size() > MAX_RETIRED_REQUESTS) {
                String oldest = retired.iterator().next();
                retired.remove(oldest);
            }
        }
    }

    static final class QueryOperation {
        private final JdbcSession session;
        private final Connection connection;
        private final RequestMeta meta;
        private final long deadlineUnixMillis;

        private int credits;
        private int reservedCredits;
        private boolean cancelled;
        private boolean terminal;
        private Statement statement;
        private SQLException cancellationFailure;
        private CancellationReason cancellationReason = CancellationReason.NONE;
        private boolean cancellationPending;
        private boolean sealed;
        private boolean ownershipDeferred;
        private boolean terminalResponseFinished;
        private boolean deferredCleanupStarted;
        private boolean deferredCleanupFinished;
        private Runnable deferredCleanup;
        private Runnable deferredCompletion;
        private volatile Future<?> future;
        private ScheduledFuture<?> deadlineWatchdog;

        private QueryOperation(
                JdbcSession session,
                Connection connection,
                RequestMeta meta,
                int initialCredits,
                long deadlineUnixMillis) {
            this.session = session;
            this.connection = connection;
            this.meta = meta;
            this.credits = initialCredits;
            this.deadlineUnixMillis = deadlineUnixMillis;
        }

        JdbcSession session() {
            return session;
        }

        Connection connection() {
            return connection;
        }

        RequestMeta meta() {
            return meta;
        }

        String requestId() {
            return meta.getRequestId();
        }

        long deadlineUnixMillis() {
            return deadlineUnixMillis;
        }

        synchronized void installStatement(Statement statement) throws RuntimeFailure {
            this.statement = statement;
            checkCancellation();
        }

        synchronized void clearStatement(Statement statement) {
            if (this.statement == statement) {
                this.statement = null;
            }
        }

        synchronized void awaitCredit() throws RuntimeFailure {
            while (credits == 0 && !cancelled && !terminal) {
                long waitMillis = 1000;
                if (deadlineUnixMillis > 0) {
                    long remaining = deadlineUnixMillis - System.currentTimeMillis();
                    if (remaining <= 0) {
                        cancelled = true;
                        cancellationReason = CancellationReason.DEADLINE;
                        throw RuntimeFailure.deadline("the query deadline elapsed while waiting for credit");
                    }
                    waitMillis = Math.min(waitMillis, remaining);
                }
                try {
                    wait(waitMillis);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    cancelled = true;
                    throw RuntimeFailure.cancelled("the query worker was interrupted");
                }
            }
            checkCancellation();
            if (terminal) {
                throw RuntimeFailure.cancelled("the query is already terminal");
            }
            credits--;
            reservedCredits++;
        }

        synchronized void returnCredit() {
            if (reservedCredits <= 0) {
                throw new IllegalStateException("no reserved query credit is available to return");
            }
            reservedCredits--;
            credits++;
            notifyAll();
        }

        synchronized void consumeReservedCredit() {
            if (reservedCredits <= 0) {
                throw new IllegalStateException("no reserved query credit is available to consume");
            }
            reservedCredits--;
        }

        synchronized void checkDeadlineAndCancellation() throws RuntimeFailure {
            if (!cancelled
                    && deadlineUnixMillis > 0
                    && System.currentTimeMillis() >= deadlineUnixMillis) {
                cancelled = true;
                cancellationReason = CancellationReason.DEADLINE;
            }
            checkCancellation();
        }

        synchronized void addCredits(int additional) throws RuntimeFailure {
            if (terminal || sealed) {
                throw RuntimeFailure.validation(
                        "operation.not_active", "target_request_id is already terminal");
            }
            if (credits + reservedCredits
                    > ProtocolLimits.MAX_OUTSTANDING_CREDITS - additional) {
                throw RuntimeFailure.validation(
                        "operation.credit_overflow",
                        "outstanding batch credits exceed the protocol limit");
            }
            credits += additional;
            notifyAll();
        }

        synchronized CancellationRequest requestCancellation(CancellationReason reason) {
            if (terminal || sealed) {
                return new CancellationRequest(false, null);
            }
            if (!cancelled) {
                cancelled = true;
                cancellationReason = reason;
            }
            Statement target = null;
            if (statement != null && !cancellationPending) {
                cancellationPending = true;
                target = statement;
            }
            notifyAll();
            return new CancellationRequest(true, target);
        }

        void cancelInstalledStatement(Statement target) {
            SQLException failure = null;
            try {
                target.cancel();
            } catch (SQLException cancelledFailure) {
                failure = cancelledFailure;
            } catch (RuntimeException cancelledFailure) {
                failure = new SQLException(
                        "the JDBC driver failed while cancelling the statement",
                        cancelledFailure);
            }
            Runnable ready;
            synchronized (this) {
                if (failure != null) {
                    cancellationFailure = failure;
                }
                cancellationPending = false;
                notifyAll();
                ready = takeDeferredCleanupIfReady();
            }
            runDeferredCleanup(ready);
        }

        void recordCancellationFailure(SQLException failure) {
            Runnable ready;
            synchronized (this) {
                cancellationFailure = failure;
                cancellationPending = false;
                notifyAll();
                ready = takeDeferredCleanupIfReady();
            }
            runDeferredCleanup(ready);
        }

        synchronized boolean hasPendingCancellation() {
            return cancellationPending;
        }

        void deferOwnershipRelease(Runnable cleanup, Runnable completion) {
            Runnable ready;
            synchronized (this) {
                ownershipDeferred = true;
                deferredCleanup = cleanup;
                deferredCompletion = completion;
                ready = takeDeferredCleanupIfReady();
            }
            runDeferredCleanup(ready);
        }

        void markTerminalResponseFinished() {
            Runnable ready;
            synchronized (this) {
                terminalResponseFinished = true;
                ready = takeDeferredCleanupIfReady();
            }
            runDeferredCleanup(ready);
        }

        synchronized boolean completionDeferred() {
            return ownershipDeferred && !deferredCleanupFinished;
        }

        synchronized void sealAndAwaitCancellation(Duration timeout) throws RuntimeFailure {
            if (!cancelled
                    && deadlineUnixMillis > 0
                    && System.currentTimeMillis() >= deadlineUnixMillis) {
                cancelled = true;
                cancellationReason = CancellationReason.DEADLINE;
            }
            sealed = true;
            if (deadlineWatchdog != null) {
                deadlineWatchdog.cancel(false);
            }
            long deadlineNanos = System.nanoTime() + Math.max(0, timeout.toNanos());
            while (cancellationPending) {
                long remainingNanos = deadlineNanos - System.nanoTime();
                if (remainingNanos <= 0) {
                    session.markBroken();
                    throw RuntimeFailure.database(
                            "database.cancel_timeout",
                            "the driver did not finish cancelling the statement and the session is broken",
                            new SQLException("the cancellation worker did not quiesce"),
                            ai.chat2db.rust.compat.protocol.v1.OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                            false).withSessionState(session.protocolState());
                }
                try {
                    long millis = Math.max(1, TimeUnit.NANOSECONDS.toMillis(remainingNanos));
                    wait(millis);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    session.markBroken();
                    throw RuntimeFailure.cancelled(
                            "the query worker was interrupted while waiting for cancellation")
                            .withOutcome(ai.chat2db.rust.compat.protocol.v1.OperationOutcome.OPERATION_OUTCOME_UNKNOWN)
                            .withSessionState(session.protocolState());
                }
            }
            checkCancellation();
        }

        synchronized void markTerminal() {
            terminal = true;
            sealed = true;
            statement = null;
            if (deadlineWatchdog != null) {
                deadlineWatchdog.cancel(false);
            }
            notifyAll();
        }

        void setFuture(Future<?> future) {
            this.future = future;
        }

        synchronized void setDeadlineWatchdog(ScheduledFuture<?> watchdog) {
            deadlineWatchdog = watchdog;
            if (terminal || sealed) {
                watchdog.cancel(false);
            }
        }

        Future<?> future() {
            return future;
        }

        private Runnable takeDeferredCleanupIfReady() {
            if (!ownershipDeferred
                    || !terminalResponseFinished
                    || cancellationPending
                    || deferredCleanupStarted) {
                return null;
            }
            deferredCleanupStarted = true;
            Runnable cleanup = deferredCleanup;
            Runnable completion = deferredCompletion;
            return () -> {
                try {
                    cleanup.run();
                } finally {
                    synchronized (QueryOperation.this) {
                        deferredCleanupFinished = true;
                        deferredCleanup = null;
                        deferredCompletion = null;
                    }
                    completion.run();
                }
            };
        }

        private static void runDeferredCleanup(Runnable cleanup) {
            if (cleanup != null) {
                cleanup.run();
            }
        }

        private void checkCancellation() throws RuntimeFailure {
            if (cancellationFailure != null) {
                session.markBroken();
                throw RuntimeFailure.database(
                        "database.cancel_failed",
                        "the driver failed to cancel the active statement and the session is broken",
                        cancellationFailure,
                        ai.chat2db.rust.compat.protocol.v1.OperationOutcome.OPERATION_OUTCOME_UNKNOWN,
                        false).withSessionState(session.protocolState());
            }
            if (cancelled) {
                if (cancellationReason == CancellationReason.DEADLINE) {
                    throw RuntimeFailure.deadline("the query deadline elapsed");
                }
                throw RuntimeFailure.cancelled("the query was cancelled");
            }
        }
    }

    private enum CancellationReason {
        NONE,
        USER,
        DEADLINE
    }

    private record CancellationRequest(boolean accepted, Statement statement) {
    }
}

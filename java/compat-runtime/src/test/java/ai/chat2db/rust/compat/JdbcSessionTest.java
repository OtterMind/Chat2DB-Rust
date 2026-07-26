package ai.chat2db.rust.compat;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaList;
import ai.chat2db.rust.compat.protocol.v1.DatabaseProduct;
import ai.chat2db.rust.compat.protocol.v1.DriverArtifact;
import ai.chat2db.rust.compat.protocol.v1.LoadDriverRequest;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import ai.chat2db.rust.compat.protocol.v1.SessionState;
import ai.chat2db.rust.compat.protocol.v1.TransactionIsolation;
import com.google.protobuf.ByteString;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.sql.Connection;
import java.sql.SQLException;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class JdbcSessionTest {

    @Test
    void communityMetadataFailuresAfterClaimRequireTransactionRollback(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "getAutoCommit" -> true;
                case "getTransactionIsolation" -> Connection.TRANSACTION_READ_COMMITTED;
                case "isReadOnly" -> false;
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));
            List<RuntimeFailure> failures = List.of(
                    RuntimeFailure.validation(
                            "community.metadata_validation_after_claim", "validation failure"),
                    RuntimeFailure.limit("community schemas", 1),
                    RuntimeFailure.internal(
                            "community.metadata_internal_after_claim",
                            "internal failure",
                            new IllegalStateException("metadata projection failed")));

            for (int index = 0; index < failures.size(); index++) {
                RuntimeFailure expected = failures.get(index);
                String requestId = "community-metadata-" + index;
                JdbcSession.TransactionDescriptor transaction = session.begin(
                        TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false);

                RuntimeFailure actual = assertThrows(
                        RuntimeFailure.class,
                        () -> JdbcRuntime.invokeCommunitySchemas(
                                session,
                                requestId,
                                Optional.of(transaction.transactionId()),
                                claimed -> {
                                    assertSame(connection, claimed);
                                    throw expected;
                                }));

                assertEquals(expected.code(), actual.code());
                assertEquals(JdbcSession.State.ROLLBACK_REQUIRED, session.state());
                assertEquals(
                        SessionState.SESSION_STATE_ROLLBACK_REQUIRED,
                        actual.toEngineError().getSessionState());
                assertEquals(
                        OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                        actual.outcome());
                assertTrue(session.activeOperationId().isEmpty());
                session.rollback(transaction.transactionId());
            }

            JdbcSession.TransactionDescriptor uncheckedTransaction = session.begin(
                    TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false);
            RuntimeFailure unchecked = assertThrows(
                    RuntimeFailure.class,
                    () -> JdbcRuntime.invokeCommunitySchemas(
                            session,
                            "community-metadata-unchecked",
                            Optional.of(uncheckedTransaction.transactionId()),
                            claimed -> {
                                assertSame(connection, claimed);
                                throw new IllegalStateException("metadata projection failed");
                            }));
            assertEquals("community.metadata_failed", unchecked.code());
            assertEquals(
                    OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                    unchecked.outcome());
            assertEquals(JdbcSession.State.ROLLBACK_REQUIRED, session.state());
            session.rollback(uncheckedTransaction.transactionId());

            session.close();
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void communityMetadataClaimFailureDoesNotPolluteTransaction(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "getAutoCommit" -> true;
                case "getTransactionIsolation" -> Connection.TRANSACTION_READ_COMMITTED;
                case "isReadOnly" -> false;
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));
            JdbcSession.TransactionDescriptor transaction = session.begin(
                    TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false);
            AtomicBoolean invoked = new AtomicBoolean();

            RuntimeFailure failure = assertThrows(
                    RuntimeFailure.class,
                    () -> JdbcRuntime.invokeCommunitySchemas(
                            session,
                            "community-metadata-invalid-claim",
                            Optional.of("wrong-transaction"),
                            claimed -> {
                                invoked.set(true);
                                return CommunitySchemaList.getDefaultInstance();
                            }));

            assertEquals("transaction.id_mismatch", failure.code());
            assertEquals(OperationOutcome.OPERATION_OUTCOME_NOT_STARTED, failure.outcome());
            assertFalse(invoked.get());
            assertEquals(JdbcSession.State.IN_TRANSACTION, session.state());
            assertTrue(session.activeOperationId().isEmpty());
            session.rollback(transaction.transactionId());
            session.close();
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void uncheckedBeginFailureIsUnknownAndBreaksSession(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "setAutoCommit" -> throw new IllegalStateException("unchecked begin failure");
                case "getAutoCommit" -> true;
                case "getTransactionIsolation" -> Connection.TRANSACTION_READ_COMMITTED;
                case "isReadOnly" -> false;
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));

            RuntimeFailure failure = assertThrows(
                    RuntimeFailure.class,
                    () -> session.begin(
                            TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false));

            assertEquals("transaction.begin_outcome_unknown", failure.code());
            assertUnknownBrokenAndIdle(session, failure);
            session.close();
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void uncheckedCommitFailureIsUnknownAndBreaksSession(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "commit" -> throw new IllegalStateException("unchecked commit failure");
                case "getAutoCommit" -> true;
                case "getTransactionIsolation" -> Connection.TRANSACTION_READ_COMMITTED;
                case "isReadOnly" -> false;
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));
            JdbcSession.TransactionDescriptor transaction = session.begin(
                    TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false);

            RuntimeFailure failure = assertThrows(
                    RuntimeFailure.class,
                    () -> session.commit(transaction.transactionId()));

            assertEquals("transaction.commit_outcome_unknown", failure.code());
            assertUnknownBrokenAndIdle(session, failure);
            session.close();
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void uncheckedRollbackFailureIsUnknownAndBreaksSession(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "rollback" -> throw new IllegalStateException("unchecked rollback failure");
                case "getAutoCommit" -> true;
                case "getTransactionIsolation" -> Connection.TRANSACTION_READ_COMMITTED;
                case "isReadOnly" -> false;
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));
            JdbcSession.TransactionDescriptor transaction = session.begin(
                    TransactionIsolation.TRANSACTION_ISOLATION_DEFAULT, false);

            RuntimeFailure failure = assertThrows(
                    RuntimeFailure.class,
                    () -> session.rollback(transaction.transactionId()));

            assertEquals("transaction.rollback_outcome_unknown", failure.code());
            assertUnknownBrokenAndIdle(session, failure);
            session.close();
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void failedConnectionCloseRetainsDriverLeaseUntilAConfirmedRetry(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            DriverRegistry.DriverLease lease = registry.acquire(descriptor.driverId());
            AtomicInteger closeAttempts = new AtomicInteger();
            AtomicBoolean closed = new AtomicBoolean();
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                        case "getAutoCommit" -> true;
                        case "close" -> {
                            if (closeAttempts.incrementAndGet() == 1) {
                                throw new SQLException("first close failed");
                            }
                            closed.set(true);
                            yield null;
                        }
                        case "isClosed" -> closed.get();
                        default -> defaultValue(method.getReturnType());
                    });
            JdbcSession session = session(connection, lease);

            assertThrows(RuntimeFailure.class, session::close);
            assertEquals(JdbcSession.State.BROKEN, session.state());
            RuntimeFailure stillLeased = assertThrows(
                    RuntimeFailure.class, () -> registry.unload(descriptor.driverId()));
            assertEquals("driver.in_use", stillLeased.code());

            session.close();
            assertEquals(JdbcSession.State.CLOSED, session.state());
            registry.unload(descriptor.driverId());
            try (var entries = Files.list(snapshotRoot)) {
                assertEquals(0, entries.count());
            }
        }
    }

    @Test
    void uncheckedCloseFailureClearsOwnershipButRetainsLeaseUntilConfirmedRetry(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            AtomicInteger autoCommitChecks = new AtomicInteger();
            AtomicInteger closeAttempts = new AtomicInteger();
            AtomicBoolean closed = new AtomicBoolean();
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "getAutoCommit" -> {
                    if (autoCommitChecks.incrementAndGet() == 1) {
                        throw new IllegalStateException("unchecked auto-commit failure");
                    }
                    yield true;
                }
                case "close" -> {
                    if (closeAttempts.incrementAndGet() == 1) {
                        throw new IllegalStateException("unchecked close failure");
                    }
                    closed.set(true);
                    yield null;
                }
                case "isClosed" -> closed.get();
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));

            RuntimeFailure failure = assertThrows(RuntimeFailure.class, session::close);

            assertEquals("session.close_failed", failure.code());
            assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, failure.outcome());
            assertEquals(JdbcSession.State.BROKEN, session.state());
            assertTrue(session.activeOperationId().isEmpty());
            RuntimeFailure stillLeased = assertThrows(
                    RuntimeFailure.class, () -> registry.unload(descriptor.driverId()));
            assertEquals("driver.in_use", stillLeased.code());

            session.close();
            assertEquals(JdbcSession.State.CLOSED, session.state());
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void uncheckedRollbackDuringCloseStillReleasesLeaseAfterConnectionCloses(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            AtomicBoolean closed = new AtomicBoolean();
            Connection connection = connection((proxy, method, arguments) -> switch (method.getName()) {
                case "getAutoCommit" -> false;
                case "rollback" -> throw new IllegalStateException("unchecked rollback failure");
                case "close" -> {
                    closed.set(true);
                    yield null;
                }
                case "isClosed" -> closed.get();
                default -> defaultValue(method.getReturnType());
            });
            JdbcSession session = session(connection, registry.acquire(descriptor.driverId()));

            RuntimeFailure failure = assertThrows(RuntimeFailure.class, session::close);

            assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, failure.outcome());
            assertEquals(JdbcSession.State.CLOSED, session.state());
            assertTrue(session.activeOperationId().isEmpty());
            registry.unload(descriptor.driverId());
        }
    }

    @Test
    void failedOpenCleanupReleasesLeaseWhenConnectionCloseThrowsUnchecked(
            @TempDir Path temporaryDirectory) throws Exception {
        Path snapshotRoot = Files.createDirectory(temporaryDirectory.resolve("snapshots"));
        try (DriverRegistry registry = new DriverRegistry(snapshotRoot)) {
            DriverRegistry.DriverDescriptor descriptor = loadH2(registry);
            DriverRegistry.DriverLease lease = registry.acquire(descriptor.driverId());
            AtomicBoolean closeAttempted = new AtomicBoolean();
            Connection connection = connection((proxy, method, arguments) -> {
                if (method.getName().equals("close")) {
                    closeAttempted.set(true);
                    throw new IllegalStateException("unchecked failed-open close");
                }
                return defaultValue(method.getReturnType());
            });

            SessionRegistry.cleanupFailedOpen(connection, lease);

            assertTrue(closeAttempted.get());
            registry.unload(descriptor.driverId());
        }
    }

    private static DriverRegistry.DriverDescriptor loadH2(DriverRegistry registry)
            throws Exception {
        Path h2Jar = h2DriverJar();
        return registry.load(
                LoadDriverRequest.newBuilder()
                        .setDriverClass("org.h2.Driver")
                        .addArtifacts(DriverArtifact.newBuilder()
                                .setPath(h2Jar.toString())
                                .setSha256(ByteString.copyFrom(sha256(h2Jar))))
                        .build());
    }

    private static Connection connection(InvocationHandler handler) {
        return (Connection) Proxy.newProxyInstance(
                Connection.class.getClassLoader(),
                new Class<?>[] {Connection.class},
                handler);
    }

    private static JdbcSession session(
            Connection connection, DriverRegistry.DriverLease lease) {
        return new JdbcSession(
                "session",
                connection,
                lease,
                DatabaseProduct.getDefaultInstance(),
                false,
                Connection.TRANSACTION_READ_COMMITTED,
                SensitiveDataRedactor.NONE);
    }

    private static void assertUnknownBrokenAndIdle(
            JdbcSession session, RuntimeFailure failure) {
        assertEquals(OperationOutcome.OPERATION_OUTCOME_UNKNOWN, failure.outcome());
        assertEquals(JdbcSession.State.BROKEN, session.state());
        assertTrue(session.activeOperationId().isEmpty());
    }

    private static Path h2DriverJar() throws Exception {
        Path directory = Path.of(System.getProperty("basedir"), "target", "test-drivers");
        try (var paths = Files.list(directory)) {
            return paths.filter(path -> path.getFileName().toString().startsWith("h2-"))
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .findFirst()
                    .orElseThrow()
                    .toRealPath();
        }
    }

    private static byte[] sha256(Path path) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (var input = Files.newInputStream(path)) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) != -1) {
                digest.update(buffer, 0, count);
            }
        }
        return digest.digest();
    }

    private static Object defaultValue(Class<?> type) {
        if (!type.isPrimitive()) {
            return null;
        }
        if (type == boolean.class) {
            return false;
        }
        if (type == byte.class) {
            return (byte) 0;
        }
        if (type == short.class) {
            return (short) 0;
        }
        if (type == int.class) {
            return 0;
        }
        if (type == long.class) {
            return 0L;
        }
        if (type == float.class) {
            return 0F;
        }
        if (type == double.class) {
            return 0D;
        }
        if (type == char.class) {
            return '\0';
        }
        return null;
    }
}

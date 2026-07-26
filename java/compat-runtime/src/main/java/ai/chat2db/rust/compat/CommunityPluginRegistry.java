package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDownloadUrlLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDriverConfig;
import ai.chat2db.rust.compat.protocol.v1.CommunityParsedStatement;
import ai.chat2db.rust.compat.protocol.v1.CommunityPluginCatalog;
import ai.chat2db.rust.compat.protocol.v1.CommunityPluginDescriptor;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchema;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaList;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlAnalysis;
import ai.chat2db.rust.compat.protocol.v1.JdbcProtocolLimit;
import ai.chat2db.rust.compat.protocol.v1.OperationOutcome;
import java.io.IOException;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.ServiceConfigurationError;
import java.util.ServiceLoader;
import java.util.Set;
import java.util.jar.Attributes;
import java.util.jar.JarFile;
import java.util.jar.Manifest;

/** Isolated reflective bridge to the retained Community database SPI. */
final class CommunityPluginRegistry implements AutoCloseable {

    static final String CLASSPATH_ENV = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
    static final String SOURCE_COMMIT_ENV = "CHAT2DB_COMMUNITY_SOURCE_COMMIT";
    static final int MAX_RESPONSE_PROJECTION_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES.getNumber();

    private static final String PLUGIN_INTERFACE = "ai.chat2db.spi.IPlugin";
    private static final String DB_CONFIG_CLASS =
            "ai.chat2db.community.domain.api.config.DBConfig";
    private static final String SCHEMA_CLASS =
            "ai.chat2db.community.domain.api.model.metadata.Schema";
    private static final int MAX_CLASSPATH_ARTIFACTS =
            CommunityCountLimit.COMMUNITY_COUNT_LIMIT_MAX_CLASSPATH_ARTIFACTS.getNumber();
    private static final long MAX_CLASSPATH_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_CLASSPATH_BYTES.getNumber();
    private static final int MAX_PLUGINS =
            CommunityCountLimit.COMMUNITY_COUNT_LIMIT_MAX_PLUGINS.getNumber();
    private static final int MAX_DRIVER_CONFIGS =
            CommunityCountLimit.COMMUNITY_COUNT_LIMIT_MAX_DRIVER_CONFIGS.getNumber();
    private static final int MAX_DOWNLOAD_URLS =
            CommunityDownloadUrlLimit.COMMUNITY_DOWNLOAD_URL_LIMIT_MAX_DOWNLOAD_URLS.getNumber();
    private static final int MAX_SCHEMAS =
            CommunitySchemaCountLimit.COMMUNITY_SCHEMA_COUNT_LIMIT_MAX_SCHEMAS.getNumber();
    private static final int MAX_STATEMENTS =
            CommunityCountLimit.COMMUNITY_COUNT_LIMIT_MAX_STATEMENTS.getNumber();
    private static final int MAX_DATABASE_TYPE_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES.getNumber();
    private static final int MAX_PLUGIN_NAME_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_PLUGIN_NAME_BYTES.getNumber();
    private static final int MAX_SOURCE_COMMIT_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_SOURCE_COMMIT_BYTES.getNumber();
    private static final int MAX_COMMENT_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_COMMENT_BYTES.getNumber();
    private static final int MAX_SCALAR_BYTES =
            JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_SCALAR_BYTES.getNumber();
    private static final int MAX_SQL_BYTES =
            JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_SQL_BYTES.getNumber();
    private static final int MAX_PATH_BYTES =
            JdbcProtocolLimit.JDBC_PROTOCOL_LIMIT_MAX_PATH_BYTES.getNumber();
    private static final int LENGTH_DELIMITED_FIELD_OVERHEAD_BYTES = 6;
    private static final int BOOLEAN_FIELD_OVERHEAD_BYTES = 2;

    private final String sourceCommit;
    private final URLClassLoader loader;
    private final Map<String, PluginHandle> plugins;
    private boolean closed;

    private CommunityPluginRegistry(
            String sourceCommit, URLClassLoader loader, Map<String, PluginHandle> plugins) {
        this.sourceCommit = sourceCommit;
        this.loader = loader;
        this.plugins = plugins;
    }

    static CommunityPluginRegistry openConfigured() {
        String configuredClasspath = System.getenv(CLASSPATH_ENV);
        String configuredCommit = System.getenv(SOURCE_COMMIT_ENV);
        if (configuredClasspath == null || configuredClasspath.isBlank()) {
            if (configuredCommit != null && !configuredCommit.isBlank()) {
                throw new IllegalStateException(
                        "Community source commit cannot be configured without a classpath");
            }
            return new CommunityPluginRegistry("", null, Map.of());
        }
        validateSourceCommit(configuredCommit);

        URLClassLoader loader = null;
        try {
            List<Path> paths = validateClasspath(configuredClasspath);
            URL[] urls = paths.stream().map(CommunityPluginRegistry::toUrl).toArray(URL[]::new);
            loader = new URLClassLoader(urls, ClassLoader.getPlatformClassLoader());
            Map<String, PluginHandle> plugins = discover(loader);
            return new CommunityPluginRegistry(configuredCommit, loader, Map.copyOf(plugins));
        } catch (RuntimeException
                | ReflectiveOperationException
                | LinkageError
                | ServiceConfigurationError failure) {
            closeQuietly(loader);
            throw new IllegalStateException(
                    "Community compatibility classpath could not be loaded", failure);
        }
    }

    CommunityPluginCatalog catalog() throws RuntimeFailure {
        ensureOpen();
        ProjectionBudget budget = ProjectionBudget.response();
        budget.consumeMessage();
        CommunityPluginCatalog.Builder catalog = CommunityPluginCatalog.newBuilder()
                .setSourceCommit(projectString(
                        sourceCommit,
                        MAX_SOURCE_COMMIT_BYTES,
                        "source_commit",
                        budget));
        for (PluginHandle plugin : orderedPlugins()) {
            budget.consumeMessage();
            catalog.addPlugins(descriptor(plugin, budget));
        }
        return catalog.build();
    }

    void validateSchemasRequest(String databaseType, String databaseName)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireUtf8(databaseName, MAX_SCALAR_BYTES, "database_name");
        requirePlugin(databaseType);
    }

    CommunitySchemaList schemas(
            String databaseType, Connection connection, String databaseName)
            throws RuntimeFailure {
        validateSchemasRequest(databaseType, databaseName);
        PluginHandle handle = requirePlugin(databaseType);
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Object metadata = invoke(handle.plugin(), "getDbMetaData");
            if (metadata == null) {
                throw RuntimeFailure.validation(
                        "community.metadata_not_supported",
                        "the selected Community plugin does not provide metadata");
            }
            Object result = invoke(
                    metadata,
                    "schemas",
                    new Class<?>[] {Connection.class, String.class},
                    connection,
                    databaseName);
            List<?> values = requireList(result, "schemas");
            if (values.size() > MAX_SCHEMAS) {
                throw RuntimeFailure.limit("community schemas", MAX_SCHEMAS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunitySchemaList.Builder schemas = CommunitySchemaList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                schemas.addSchemas(schema(value, budget));
            }
            return schemas.build();
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw metadataFailure(failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    CommunityBuiltSql buildCreateSchema(String databaseType, CommunitySchema requested)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        if (requested == null) {
            throw RuntimeFailure.validation(
                    "community.schema_required", "schema is required");
        }
        requireNonBlank(requested.getName(), MAX_SCALAR_BYTES, "schema_name");
        requireUtf8(requested.getDatabaseName(), MAX_SCALAR_BYTES, "schema_database_name");
        requireUtf8(requested.getComment(), MAX_COMMENT_BYTES, "schema_comment");
        requireUtf8(requested.getOwner(), MAX_SCALAR_BYTES, "schema_owner");

        PluginHandle handle = requirePlugin(databaseType);
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Object metadata = invoke(handle.plugin(), "getDbMetaData");
            Object builder = metadata == null ? null : invoke(metadata, "getSqlBuilder");
            if (builder == null) {
                throw RuntimeFailure.validation(
                        "community.sql_builder_not_supported",
                        "the selected Community plugin does not provide a SQL builder");
            }
            Object ddl = invoke(builder, "ddl");
            Object schemaBuilder = invoke(ddl, "schema");
            Class<?> schemaType = Class.forName(SCHEMA_CLASS, true, loader);
            Object communitySchema = schemaType.getDeclaredConstructor().newInstance();
            invokeSetter(communitySchema, "setDatabaseName", String.class, requested.getDatabaseName());
            invokeSetter(communitySchema, "setName", String.class, requested.getName());
            invokeSetter(communitySchema, "setComment", String.class, requested.getComment());
            invokeSetter(communitySchema, "setOwner", String.class, requested.getOwner());
            invokeSetter(communitySchema, "setSystem", boolean.class, requested.getSystem());
            Object built = invoke(
                    schemaBuilder,
                    "buildCreateSchema",
                    new Class<?>[] {schemaType},
                    communitySchema);
            String sql = scalar(built);
            requireNonBlank(sql, MAX_SQL_BYTES, "built_sql");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            return CommunityBuiltSql.newBuilder()
                    .setSql(projectString(sql, MAX_SQL_BYTES, "built_sql", budget))
                    .build();
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (InvocationTargetException failure) {
            Throwable cause = rootInvocationCause(failure);
            if (cause instanceof UnsupportedOperationException) {
                throw RuntimeFailure.validation(
                        "community.sql_builder_not_supported",
                        "the selected Community SQL builder does not support CREATE SCHEMA");
            }
            throw RuntimeFailure.internal(
                    "community.sql_builder_failed",
                    "the Community SQL builder failed",
                    cause);
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw RuntimeFailure.internal(
                    "community.sql_builder_failed",
                    "the Community SQL builder failed",
                    failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    CommunitySqlAnalysis parse(String databaseType, String sql) throws RuntimeFailure {
        ensureOpen();
        requireNonBlank(sql, MAX_SQL_BYTES, "sql");
        PluginHandle handle = requirePlugin(databaseType);
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Object syntax = invoke(handle.plugin(), "getSqlSyntaxPlugin");
            if (syntax == null) {
                throw RuntimeFailure.validation(
                        "community.sql_parser_not_supported",
                        "the selected Community plugin does not provide a SQL parser");
            }
            Object parser = invoke(syntax, "getSQLParser");
            if (parser == null) {
                throw RuntimeFailure.validation(
                        "community.sql_parser_not_supported",
                        "the selected Community plugin does not provide a SQL parser");
            }
            boolean isSelect = booleanValue(invoke(
                    parser, "isSelect", new Class<?>[] {String.class}, sql));
            List<?> statements = requireList(
                    invoke(parser, "parserSqlScript", new Class<?>[] {String.class}, sql),
                    "statements");
            if (statements.size() > MAX_STATEMENTS) {
                throw RuntimeFailure.limit("community parsed statements", MAX_STATEMENTS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            budget.consumeBoolean();
            CommunitySqlAnalysis.Builder analysis =
                    CommunitySqlAnalysis.newBuilder().setIsSelect(isSelect);
            for (Object statement : statements) {
                budget.consumeMessage();
                analysis.addStatements(CommunityParsedStatement.newBuilder()
                        .setSql(getString(
                                statement,
                                "getSql",
                                MAX_SQL_BYTES,
                                "statement_sql",
                                budget))
                        .setType(getString(
                                statement,
                                "getType",
                                MAX_SCALAR_BYTES,
                                "statement_type",
                                budget))
                        .setStatementType(getString(
                                statement,
                                "getStatementType",
                                MAX_SCALAR_BYTES,
                                "statement_statement_type",
                                budget)));
            }
            return analysis.build();
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (InvocationTargetException failure) {
            throw RuntimeFailure.internal(
                    "community.sql_parser_failed",
                    "the Community SQL parser failed internally",
                    rootInvocationCause(failure));
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw RuntimeFailure.internal(
                    "community.sql_parser_failed",
                    "the Community SQL parser failed internally",
                    failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        closed = true;
        closeQuietly(loader);
    }

    private List<PluginHandle> orderedPlugins() {
        return plugins.values().stream()
                .sorted(Comparator.comparing(PluginHandle::databaseType))
                .toList();
    }

    private CommunityPluginDescriptor descriptor(PluginHandle handle, ProjectionBudget budget)
            throws RuntimeFailure {
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Object config = handle.config();
            CommunityPluginDescriptor.Builder descriptor = CommunityPluginDescriptor.newBuilder()
                    .setDatabaseType(projectString(
                            handle.databaseType(),
                            MAX_DATABASE_TYPE_BYTES,
                            "database_type",
                            budget))
                    .setName(getString(
                            config,
                            "getName",
                            MAX_PLUGIN_NAME_BYTES,
                            "plugin_name",
                            budget))
                    .setSupportsDatabase(getBoolean(config, "isSupportDatabase"))
                    .setSupportsSchema(getBoolean(config, "isSupportSchema"))
                    .setPreservesScriptBatchExecution(
                            getBoolean(config, "isPreserveScriptBatchExecution"));
            budget.consumeBooleans(3);
            List<?> drivers = requireList(invoke(config, "getDriverConfigList"), "driver configs");
            if (drivers.size() > MAX_DRIVER_CONFIGS) {
                throw RuntimeFailure.limit("community driver configs", MAX_DRIVER_CONFIGS);
            }
            for (Object driver : drivers) {
                budget.consumeMessage();
                descriptor.addDrivers(driver(driver, budget));
            }
            Object metadata = invoke(handle.plugin(), "getDbMetaData");
            Object builder = metadata == null ? null : invoke(metadata, "getSqlBuilder");
            Object syntax = invoke(handle.plugin(), "getSqlSyntaxPlugin");
            budget.consumeBooleans(3);
            descriptor.setMetadataAvailable(metadata != null);
            descriptor.setSqlBuilderAvailable(builder != null);
            descriptor.setSqlParserAvailable(syntax != null && invoke(syntax, "getSQLParser") != null);
            return descriptor.build();
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw RuntimeFailure.internal(
                    "community.plugin_projection_failed",
                    "the Community plugin catalog could not be projected",
                    failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    private CommunityDriverConfig driver(Object driver, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityDriverConfig.Builder result = CommunityDriverConfig.newBuilder()
                .setUrl(getString(
                        driver,
                        "getUrl",
                        MAX_SCALAR_BYTES,
                        "driver_url",
                        budget))
                .setJdbcDriver(getString(
                        driver,
                        "getJdbcDriver",
                        MAX_SCALAR_BYTES,
                        "jdbc_driver",
                        budget))
                .setJdbcDriverClass(getString(
                        driver,
                        "getJdbcDriverClass",
                        MAX_SCALAR_BYTES,
                        "jdbc_driver_class",
                        budget))
                .setCustom(getBoolean(driver, "isCustom"))
                .setDefaultDriver(getBoolean(driver, "isDefaultDriver"));
        budget.consumeBooleans(2);
        List<?> urls = requireList(
                invoke(driver, "getDownloadJdbcDriverUrls"), "download URLs");
        if (urls.size() > MAX_DOWNLOAD_URLS) {
            throw RuntimeFailure.limit("community driver download URLs", MAX_DOWNLOAD_URLS);
        }
        for (Object url : urls) {
            String value = scalar(url);
            result.addDownloadUrls(projectString(
                    value,
                    MAX_SCALAR_BYTES,
                    "driver_download_url",
                    budget));
        }
        return result.build();
    }

    private CommunitySchema schema(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunitySchema.Builder schema = CommunitySchema.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "schema_database_name",
                        budget))
                .setName(getString(
                        value,
                        "getName",
                        MAX_SCALAR_BYTES,
                        "schema_name",
                        budget))
                .setComment(getString(
                        value,
                        "getComment",
                        MAX_COMMENT_BYTES,
                        "schema_comment",
                        budget))
                .setOwner(getString(
                        value,
                        "getOwner",
                        MAX_SCALAR_BYTES,
                        "schema_owner",
                        budget))
                .setSystem(getBoolean(value, "isSystem"));
        budget.consumeBoolean();
        return schema.build();
    }

    private PluginHandle requirePlugin(String databaseType) throws RuntimeFailure {
        requireDatabaseType(databaseType);
        PluginHandle plugin = plugins.get(normalize(databaseType));
        if (plugin == null) {
            throw RuntimeFailure.validation(
                    "community.plugin_not_found",
                    "the requested Community database plugin is not installed");
        }
        return plugin;
    }

    private void ensureOpen() throws RuntimeFailure {
        if (closed) {
            throw RuntimeFailure.conflict(
                    "community.registry_closed", "the Community plugin registry is closed");
        }
    }

    private static Map<String, PluginHandle> discover(URLClassLoader loader)
            throws ReflectiveOperationException {
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Class<?> pluginType = Class.forName(PLUGIN_INTERFACE, true, loader);
            Class<?> dbConfigType = Class.forName(DB_CONFIG_CLASS, true, loader);
            @SuppressWarnings({"rawtypes", "unchecked"})
            ServiceLoader<?> services = ServiceLoader.load((Class) pluginType, loader);
            Map<String, PluginHandle> plugins = new HashMap<>();
            for (Object candidate : services) {
                Object config = invoke(candidate, "getDBConfig");
                if (config != null) {
                    register(plugins, candidate, config);
                    continue;
                }
                List<?> configs = requireListForStartup(
                        invoke(candidate, "getDBConfigList"), "database configs");
                for (Object listedConfig : configs) {
                    Object selected = invoke(
                            candidate,
                            "getPlugin",
                            new Class<?>[] {dbConfigType},
                            listedConfig);
                    if (selected == null) {
                        throw new IllegalStateException(
                                "Community plugin factory returned null");
                    }
                    register(plugins, selected, listedConfig);
                }
            }
            return plugins;
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    private static void register(Map<String, PluginHandle> plugins, Object plugin, Object config)
            throws ReflectiveOperationException {
        String databaseType = scalar(invoke(config, "getDbType"));
        if (databaseType.isBlank()
                || ProtocolLimits.utf8LengthExceeds(databaseType, MAX_DATABASE_TYPE_BYTES)) {
            throw new IllegalStateException("Community plugin database type is invalid");
        }
        String key = normalize(databaseType);
        if (plugins.putIfAbsent(key, new PluginHandle(databaseType, plugin, config)) != null) {
            throw new IllegalStateException(
                    "duplicate Community plugin database type: " + databaseType);
        }
        if (plugins.size() > MAX_PLUGINS) {
            throw new IllegalStateException(
                    "Community plugin count exceeds " + MAX_PLUGINS);
        }
    }

    static List<Path> validateClasspath(String configuredDirectory) {
        if (configuredDirectory.isBlank()
                || ProtocolLimits.utf8LengthExceeds(configuredDirectory, MAX_PATH_BYTES)) {
            throw new IllegalStateException("Community classpath directory is invalid");
        }
        try {
            Path supplied = Path.of(configuredDirectory);
            if (!supplied.isAbsolute() || Files.isSymbolicLink(supplied)) {
                throw new IllegalStateException(
                        "Community classpath directory must be absolute and non-symbolic");
            }
            Path directory = supplied.toRealPath(LinkOption.NOFOLLOW_LINKS);
            if (!supplied.normalize().equals(directory)
                    || !Files.isDirectory(directory, LinkOption.NOFOLLOW_LINKS)
                    || !Files.isReadable(directory)) {
                throw new IllegalStateException(
                        "Community classpath directory must be canonical and readable");
            }

            List<Path> entries;
            try (var stream = Files.list(directory)) {
                entries = stream.toList();
            }
            if (entries.isEmpty() || entries.size() > MAX_CLASSPATH_ARTIFACTS) {
                throw new IllegalStateException(
                        "Community classpath artifact count is invalid");
            }

            List<Path> paths = new ArrayList<>(entries.size());
            Set<Path> identities = new HashSet<>();
            long totalBytes = 0;
            for (Path entry : entries) {
                if (Files.isSymbolicLink(entry)) {
                    throw new IllegalStateException(
                            "Community classpath entries must be non-symbolic JARs");
                }
                Path canonical = entry.toRealPath(LinkOption.NOFOLLOW_LINKS);
                if (!entry.normalize().equals(canonical)
                        || !Files.isRegularFile(canonical, LinkOption.NOFOLLOW_LINKS)
                        || !Files.isReadable(canonical)
                        || !canonical.getFileName().toString().toLowerCase(Locale.ROOT).endsWith(".jar")) {
                    throw new IllegalStateException(
                            "Community classpath entries must be canonical readable JARs");
                }
                if (!identities.add(canonical)) {
                    throw new IllegalStateException("Community classpath contains a duplicate JAR");
                }
                totalBytes = Math.addExact(totalBytes, Files.size(canonical));
                if (totalBytes > MAX_CLASSPATH_BYTES) {
                    throw new IllegalStateException(
                            "Community classpath exceeds its byte limit");
                }
                rejectManifestClassPath(canonical);
                paths.add(canonical);
            }
            paths.sort(Comparator.comparing(Path::toString));
            return paths;
        } catch (IOException | ArithmeticException failure) {
            throw new IllegalStateException(
                    "Community classpath directory could not be validated", failure);
        }
    }

    private static void rejectManifestClassPath(Path path) throws IOException {
        try (JarFile jar = new JarFile(path.toFile(), true)) {
            Manifest manifest = jar.getManifest();
            if (manifest == null) {
                return;
            }
            String classPath = manifest.getMainAttributes().getValue(Attributes.Name.CLASS_PATH);
            if (classPath != null && !classPath.isBlank()) {
                throw new IllegalStateException(
                        "Community JAR manifests must not declare Class-Path");
            }
        }
    }

    private static void validateSourceCommit(String commit) {
        if (commit == null
                || commit.length() != 40
                || ProtocolLimits.utf8LengthExceeds(commit, MAX_SOURCE_COMMIT_BYTES)
                || !commit.chars().allMatch(character ->
                        character >= '0' && character <= '9'
                                || character >= 'a' && character <= 'f')) {
            throw new IllegalStateException(
                    "Community source commit must be a 40-character lowercase Git SHA");
        }
    }

    private static RuntimeFailure metadataFailure(Throwable failure) {
        SQLException database = findSqlException(failure);
        if (database != null) {
            return RuntimeFailure.database(
                    "community.metadata_failed",
                    "the Community metadata request failed",
                    database,
                    OperationOutcome.OPERATION_OUTCOME_KNOWN_FAILED,
                    false);
        }
        return RuntimeFailure.internal(
                "community.metadata_failed",
                "the Community metadata request failed internally",
                rootInvocationCause(failure));
    }

    private static SQLException findSqlException(Throwable failure) {
        Throwable current = rootInvocationCause(failure);
        Set<Throwable> seen = java.util.Collections.newSetFromMap(new java.util.IdentityHashMap<>());
        while (current != null && seen.add(current)) {
            if (current instanceof SQLException sqlFailure) {
                return sqlFailure;
            }
            current = current.getCause();
        }
        return null;
    }

    private static Throwable rootInvocationCause(Throwable failure) {
        if (failure instanceof InvocationTargetException invocation
                && invocation.getCause() != null) {
            return invocation.getCause();
        }
        return failure;
    }

    private static Object invoke(Object target, String method)
            throws ReflectiveOperationException {
        return invoke(target, method, new Class<?>[0]);
    }

    private static Object invoke(
            Object target, String method, Class<?>[] parameterTypes, Object... arguments)
            throws ReflectiveOperationException {
        if (target == null) {
            throw new IllegalStateException("Community invocation target is null");
        }
        Method reflected = target.getClass().getMethod(method, parameterTypes);
        return reflected.invoke(target, arguments);
    }

    private static void invokeSetter(
            Object target, String method, Class<?> parameterType, Object value)
            throws ReflectiveOperationException {
        invoke(target, method, new Class<?>[] {parameterType}, value);
    }

    private static String getString(
            Object target,
            String getter,
            int maximum,
            String field,
            ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        String value = scalar(invoke(target, getter));
        return projectString(value, maximum, field, budget);
    }

    private static String projectString(
            String value, int maximum, String field, ProjectionBudget budget)
            throws RuntimeFailure {
        requireUtf8(value, maximum, field);
        if (budget != null) {
            budget.consumeUtf8(value);
        }
        return value;
    }

    private static boolean getBoolean(Object target, String getter)
            throws ReflectiveOperationException {
        return booleanValue(invoke(target, getter));
    }

    private static boolean booleanValue(Object value) {
        if (value instanceof Boolean booleanValue) {
            return booleanValue;
        }
        throw new IllegalStateException("Community plugin returned a non-boolean value");
    }

    private static String scalar(Object value) {
        return value == null ? "" : value.toString();
    }

    private static List<?> requireList(Object value, String field) throws RuntimeFailure {
        if (value == null) {
            return List.of();
        }
        if (value instanceof List<?> list) {
            return list;
        }
        throw RuntimeFailure.internal(
                "community.invalid_projection",
                "the Community plugin returned an invalid " + field + " projection",
                new IllegalStateException("expected a List"));
    }

    private static List<?> requireListForStartup(Object value, String field) {
        if (value == null) {
            return List.of();
        }
        if (value instanceof List<?> list) {
            return list;
        }
        throw new IllegalStateException(
                "Community plugin returned invalid " + field);
    }

    private static void requireDatabaseType(String databaseType) throws RuntimeFailure {
        requireNonBlank(databaseType, MAX_DATABASE_TYPE_BYTES, "database_type");
    }

    private static void requireNonBlank(String value, int maximum, String field)
            throws RuntimeFailure {
        if (value == null || value.isBlank()) {
            throw RuntimeFailure.validation(
                    "protocol.invalid_" + field, field + " is required");
        }
        requireUtf8(value, maximum, field);
    }

    private static void requireUtf8(String value, int maximum, String field)
            throws RuntimeFailure {
        ProtocolLimits.requireUtf8(value == null ? "" : value, maximum, field);
    }

    private static String normalize(String databaseType) {
        return databaseType.trim().toUpperCase(Locale.ROOT);
    }

    private static URL toUrl(Path path) {
        try {
            return path.toUri().toURL();
        } catch (IOException failure) {
            throw new IllegalStateException("Community classpath URL is invalid", failure);
        }
    }

    private static void closeQuietly(URLClassLoader loader) {
        if (loader == null) {
            return;
        }
        try {
            loader.close();
        } catch (IOException ignored) {
            // Process teardown remains authoritative.
        }
    }

    static final class ProjectionBudget {
        private int remainingBytes;

        private ProjectionBudget(int maximumBytes) {
            remainingBytes = maximumBytes;
        }

        static ProjectionBudget response() {
            return new ProjectionBudget(MAX_RESPONSE_PROJECTION_BYTES);
        }

        void consumeMessage() throws RuntimeFailure {
            consume(LENGTH_DELIMITED_FIELD_OVERHEAD_BYTES);
        }

        void consumeBoolean() throws RuntimeFailure {
            consume(BOOLEAN_FIELD_OVERHEAD_BYTES);
        }

        void consumeBooleans(int count) throws RuntimeFailure {
            for (int index = 0; index < count; index++) {
                consumeBoolean();
            }
        }

        void consumeUtf8(String value) throws RuntimeFailure {
            int utf8Bytes = ProtocolLimits.utf8Length(value);
            if (remainingBytes < LENGTH_DELIMITED_FIELD_OVERHEAD_BYTES
                    || utf8Bytes > remainingBytes - LENGTH_DELIMITED_FIELD_OVERHEAD_BYTES) {
                throw exceeded();
            }
            consume(LENGTH_DELIMITED_FIELD_OVERHEAD_BYTES + utf8Bytes);
        }

        private void consume(int encodedBytes) throws RuntimeFailure {
            if (encodedBytes < 0 || encodedBytes > remainingBytes) {
                throw exceeded();
            }
            remainingBytes -= encodedBytes;
        }

        private static RuntimeFailure exceeded() {
            return RuntimeFailure.limit(
                    "Community response projection",
                    MAX_RESPONSE_PROJECTION_BYTES);
        }
    }

    private record PluginHandle(String databaseType, Object plugin, Object config) {}
}

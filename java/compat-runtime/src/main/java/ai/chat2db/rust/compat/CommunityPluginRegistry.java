package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.BuildCommunityDmlRequest;
import ai.chat2db.rust.compat.protocol.v1.BuildCommunityNamespaceSqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltDml;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltNamespaceSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityBuiltSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityColumnCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabase;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabaseCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDatabaseList;
import ai.chat2db.rust.compat.protocol.v1.CommunityDownloadUrlLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityDriverConfig;
import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKey;
import ai.chat2db.rust.compat.protocol.v1.CommunityForeignKeyList;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunction;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionList;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionParameter;
import ai.chat2db.rust.compat.protocol.v1.CommunityFunctionParameterList;
import ai.chat2db.rust.compat.protocol.v1.CommunityIndexColumnCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityIndexCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityKeyCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityParsedStatement;
import ai.chat2db.rust.compat.protocol.v1.CommunityPluginCatalog;
import ai.chat2db.rust.compat.protocol.v1.CommunityPluginDescriptor;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKey;
import ai.chat2db.rust.compat.protocol.v1.CommunityPrimaryKeyList;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedure;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureList;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureParameter;
import ai.chat2db.rust.compat.protocol.v1.CommunityProcedureParameterList;
import ai.chat2db.rust.compat.protocol.v1.CommunityCreateSchemaSql;
import ai.chat2db.rust.compat.protocol.v1.CommunityRoutineParameterCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchema;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaList;
import ai.chat2db.rust.compat.protocol.v1.CommunitySchemaCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlAnalysis;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlDiagnostic;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlDiagnosticCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlValidation;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletion;
import ai.chat2db.rust.compat.protocol.v1.CompleteCommunitySqlRequest;
import ai.chat2db.rust.compat.protocol.v1.CommunityTable;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableColumn;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableColumnList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableIndex;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableIndexColumn;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableIndexList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTableList;
import ai.chat2db.rust.compat.protocol.v1.CommunityTrigger;
import ai.chat2db.rust.compat.protocol.v1.CommunityTriggerCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityTriggerList;
import ai.chat2db.rust.compat.protocol.v1.CommunityViewCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunityViewList;
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
    private static final String TABLES_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.TablesRequest";
    private static final String TABLE_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.TableMetadataRequest";
    private static final String VIEW_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.ViewMetadataRequest";
    private static final String FUNCTION_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.FunctionMetadataRequest";
    private static final String PROCEDURE_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.ProcedureMetadataRequest";
    private static final String TRIGGER_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.TriggerMetadataRequest";
    private static final String H2_DATABASE_TYPE = "H2";
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
    private static final int MAX_DATABASES =
            CommunityDatabaseCountLimit.COMMUNITY_DATABASE_COUNT_LIMIT_MAX_DATABASES.getNumber();
    private static final int MAX_TABLES =
            CommunityTableCountLimit.COMMUNITY_TABLE_COUNT_LIMIT_MAX_TABLES.getNumber();
    private static final int MAX_VIEWS =
            CommunityViewCountLimit.COMMUNITY_VIEW_COUNT_LIMIT_MAX_VIEWS.getNumber();
    private static final int MAX_KEYS =
            CommunityKeyCountLimit.COMMUNITY_KEY_COUNT_LIMIT_MAX_KEYS.getNumber();
    private static final int MAX_FUNCTIONS =
            CommunityFunctionCountLimit.COMMUNITY_FUNCTION_COUNT_LIMIT_MAX_FUNCTIONS.getNumber();
    private static final int MAX_PROCEDURES =
            CommunityProcedureCountLimit.COMMUNITY_PROCEDURE_COUNT_LIMIT_MAX_PROCEDURES.getNumber();
    private static final int MAX_TRIGGERS =
            CommunityTriggerCountLimit.COMMUNITY_TRIGGER_COUNT_LIMIT_MAX_TRIGGERS.getNumber();
    private static final int MAX_ROUTINE_PARAMETERS =
            CommunityRoutineParameterCountLimit
                    .COMMUNITY_ROUTINE_PARAMETER_COUNT_LIMIT_MAX_PARAMETERS
                    .getNumber();
    private static final int MAX_COLUMNS =
            CommunityColumnCountLimit.COMMUNITY_COLUMN_COUNT_LIMIT_MAX_COLUMNS.getNumber();
    private static final int MAX_INDEXES =
            CommunityIndexCountLimit.COMMUNITY_INDEX_COUNT_LIMIT_MAX_INDEXES.getNumber();
    private static final int MAX_INDEX_COLUMNS =
            CommunityIndexColumnCountLimit.COMMUNITY_INDEX_COLUMN_COUNT_LIMIT_MAX_INDEX_COLUMNS
                    .getNumber();
    private static final int MAX_STATEMENTS =
            CommunityCountLimit.COMMUNITY_COUNT_LIMIT_MAX_STATEMENTS.getNumber();
    static final int MAX_SQL_DIAGNOSTICS =
            CommunitySqlDiagnosticCountLimit
                    .COMMUNITY_SQL_DIAGNOSTIC_COUNT_LIMIT_MAX_DIAGNOSTICS
                    .getNumber();
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
    private static final int NUMERIC_FIELD_OVERHEAD_BYTES = 15;

    private final String sourceCommit;
    private final URLClassLoader loader;
    private final Map<String, PluginHandle> plugins;
    private final CommunitySqlCompletionBridge sqlCompletion;
    private final CommunityDmlBuilder dmlBuilder;
    private final CommunityNamespaceBuilder namespaceBuilder;
    private boolean closed;

    private CommunityPluginRegistry(
            String sourceCommit, URLClassLoader loader, Map<String, PluginHandle> plugins) {
        this(sourceCommit, loader, plugins, null);
    }

    private CommunityPluginRegistry(
            String sourceCommit,
            URLClassLoader loader,
            Map<String, PluginHandle> plugins,
            CommunitySqlCompletionBridge sqlCompletion) {
        this.sourceCommit = sourceCommit;
        this.loader = loader;
        this.plugins = plugins;
        this.sqlCompletion = sqlCompletion;
        this.dmlBuilder = loader == null ? null : new CommunityDmlBuilder(loader);
        this.namespaceBuilder = loader == null ? null : new CommunityNamespaceBuilder(loader);
    }

    static CommunityPluginRegistry openConfigured() {
        String configuredClasspath = System.getenv(CLASSPATH_ENV);
        String configuredCommit = System.getenv(SOURCE_COMMIT_ENV);
        if (configuredClasspath == null || configuredClasspath.isBlank()) {
            if (configuredCommit != null && !configuredCommit.isBlank()) {
                throw new IllegalStateException(
                        "Community source commit cannot be configured without a classpath");
            }
            return new CommunityPluginRegistry("", null, Map.of(), null);
        }
        validateSourceCommit(configuredCommit);

        URLClassLoader loader = null;
        try {
            List<Path> paths = validateClasspath(configuredClasspath);
            URL[] urls = paths.stream().map(CommunityPluginRegistry::toUrl).toArray(URL[]::new);
            loader = new URLClassLoader(urls, ClassLoader.getPlatformClassLoader());
            Map<String, PluginHandle> plugins = discover(loader);
            CommunitySqlCompletionBridge sqlCompletion = CommunitySqlCompletionBridge.open(loader);
            return new CommunityPluginRegistry(
                    configuredCommit, loader, Map.copyOf(plugins), sqlCompletion);
        } catch (RuntimeException
                | ReflectiveOperationException
                | LinkageError
                | ServiceConfigurationError failure) {
            closeQuietly(loader);
            throw new IllegalStateException(
                    "Community compatibility classpath could not be loaded", failure);
        }
    }

    boolean configured() {
        return loader != null;
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

    void validateSqlCompletionRequest(CompleteCommunitySqlRequest request)
            throws RuntimeFailure {
        ensureOpen();
        requirePlugin(request.getDatabaseType());
        requireSqlCompletion().validateRequest(request);
    }

    CommunitySqlCompletion completeSql(
            Connection connection, CompleteCommunitySqlRequest request)
            throws RuntimeFailure {
        ensureOpen();
        PluginHandle handle = requirePlugin(request.getDatabaseType());
        return requireSqlCompletion().complete(handle.databaseType(), connection, request);
    }

    private CommunitySqlCompletionBridge requireSqlCompletion() throws RuntimeFailure {
        if (sqlCompletion == null) {
            throw RuntimeFailure.conflict(
                    "community.sql_completion_unavailable",
                    "Community SQL completion is not configured");
        }
        return sqlCompletion;
    }

    synchronized CommunityBuiltDml buildDml(BuildCommunityDmlRequest request)
            throws RuntimeFailure {
        ensureOpen();
        CommunityDmlBuilder.validateRequest(request);
        PluginHandle handle = requirePlugin(request.getDatabaseType());
        if (dmlBuilder == null) {
            throw RuntimeFailure.conflict(
                    "community.dml_builder_unavailable",
                    "Community DML generation is not configured");
        }
        return dmlBuilder.build(handle.plugin(), request);
    }

    synchronized CommunityBuiltNamespaceSql buildNamespace(
            BuildCommunityNamespaceSqlRequest request) throws RuntimeFailure {
        ensureOpen();
        CommunityNamespaceBuilder.validateRequest(request);
        PluginHandle handle = requirePlugin(request.getDatabaseType());
        if (namespaceBuilder == null) {
            throw RuntimeFailure.conflict(
                    "community.namespace_builder_unavailable",
                    "Community namespace generation is not configured");
        }
        return namespaceBuilder.build(handle.plugin(), request);
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
        return withMetadata(databaseType, metadata -> {
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
        });
    }

    void validateDatabasesRequest(String databaseType) throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requirePlugin(databaseType);
    }

    CommunityDatabaseList databases(String databaseType, Connection connection)
            throws RuntimeFailure {
        validateDatabasesRequest(databaseType);
        return withMetadata(databaseType, metadata -> {
            List<?> values = requireList(
                    invoke(metadata, "databases", new Class<?>[] {Connection.class}, connection),
                    "databases");
            if (values.size() > MAX_DATABASES) {
                throw RuntimeFailure.limit("community databases", MAX_DATABASES);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityDatabaseList.Builder databases = CommunityDatabaseList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                databases.addDatabases(database(value, budget));
            }
            return databases.build();
        });
    }

    void validateTablesRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String tableNamePattern)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireUtf8(databaseName, MAX_SCALAR_BYTES, "database_name");
        requireUtf8(schemaName, MAX_SCALAR_BYTES, "schema_name");
        requireUtf8(tableNamePattern, MAX_SCALAR_BYTES, "table_name_pattern");
        requirePlugin(databaseType);
    }

    CommunityTableList tables(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableNamePattern)
            throws RuntimeFailure {
        validateTablesRequest(databaseType, databaseName, schemaName, tableNamePattern);
        return withMetadata(databaseType, metadata -> {
            Object request = metadataRequest(
                    TABLES_REQUEST_CLASS, databaseName, schemaName, tableNamePattern);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "tables",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "tables");
            if (values.size() > MAX_TABLES) {
                throw RuntimeFailure.limit("community tables", MAX_TABLES);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityTableList.Builder tables = CommunityTableList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                tables.addTables(table(value, budget));
            }
            return tables.build();
        });
    }

    void validateViewsRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String viewNamePattern)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireUtf8(databaseName, MAX_SCALAR_BYTES, "database_name");
        requireUtf8(schemaName, MAX_SCALAR_BYTES, "schema_name");
        requireUtf8(viewNamePattern, MAX_SCALAR_BYTES, "view_name_pattern");
        requirePlugin(databaseType);
    }

    CommunityViewList views(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String viewNamePattern)
            throws RuntimeFailure {
        validateViewsRequest(databaseType, databaseName, schemaName, viewNamePattern);
        return withMetadata(databaseType, metadata -> {
            Object result;
            if (viewNamePattern.isBlank()) {
                result = invoke(
                        metadata,
                        "views",
                        new Class<?>[] {Connection.class, String.class, String.class},
                        connection,
                        databaseName,
                        schemaName);
            } else {
                Object request = viewMetadataRequest(databaseName, schemaName, viewNamePattern);
                result = invoke(
                        metadata,
                        "views",
                        new Class<?>[] {Connection.class, request.getClass()},
                        connection,
                        request);
            }
            List<?> values = requireList(result, "views");
            if (values.size() > MAX_VIEWS) {
                throw RuntimeFailure.limit("community views", MAX_VIEWS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityViewList.Builder views = CommunityViewList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                views.addViews(table(value, budget));
            }
            return views.build();
        });
    }

    void validateTableObjectRequest(
            String databaseType, String databaseName, String schemaName, String tableName)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireUtf8(databaseName, MAX_SCALAR_BYTES, "database_name");
        requireUtf8(schemaName, MAX_SCALAR_BYTES, "schema_name");
        requireNonBlank(tableName, MAX_SCALAR_BYTES, "table_name");
        requirePlugin(databaseType);
    }

    CommunityTableColumnList columns(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName)
            throws RuntimeFailure {
        validateTableObjectRequest(databaseType, databaseName, schemaName, tableName);
        return withMetadata(databaseType, metadata -> {
            Object request = metadataRequest(
                    TABLE_METADATA_REQUEST_CLASS, databaseName, schemaName, tableName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "columns",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "columns");
            if (values.size() > MAX_COLUMNS) {
                throw RuntimeFailure.limit("community columns", MAX_COLUMNS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityTableColumnList.Builder columns = CommunityTableColumnList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                columns.addColumns(column(value, budget));
            }
            return columns.build();
        });
    }

    CommunityTableIndexList indexes(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName)
            throws RuntimeFailure {
        validateTableObjectRequest(databaseType, databaseName, schemaName, tableName);
        return withMetadata(databaseType, metadata -> {
            Object request = metadataRequest(
                    TABLE_METADATA_REQUEST_CLASS, databaseName, schemaName, tableName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "indexes",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "indexes");
            if (values.size() > MAX_INDEXES) {
                throw RuntimeFailure.limit("community indexes", MAX_INDEXES);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            int[] projectedColumns = {0};
            CommunityTableIndexList.Builder indexes = CommunityTableIndexList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                indexes.addIndexes(index(value, budget, projectedColumns));
            }
            return indexes.build();
        });
    }

    CommunityForeignKeyList importedKeys(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName)
            throws RuntimeFailure {
        return foreignKeys(
                databaseType,
                connection,
                databaseName,
                schemaName,
                tableName,
                "getImportedKeys",
                "imported keys");
    }

    CommunityForeignKeyList exportedKeys(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName)
            throws RuntimeFailure {
        return foreignKeys(
                databaseType,
                connection,
                databaseName,
                schemaName,
                tableName,
                "getExportedKeys",
                "exported keys");
    }

    private CommunityForeignKeyList foreignKeys(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName,
            String method,
            String field)
            throws RuntimeFailure {
        validateTableObjectRequest(databaseType, databaseName, schemaName, tableName);
        return withMetadata(databaseType, metadata -> {
            Object request = metadataRequest(
                    TABLE_METADATA_REQUEST_CLASS, databaseName, schemaName, tableName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            method,
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    field);
            if (values.size() > MAX_KEYS) {
                throw RuntimeFailure.limit("community " + field, MAX_KEYS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityForeignKeyList.Builder keys = CommunityForeignKeyList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                keys.addKeys(foreignKey(value, budget));
            }
            return keys.build();
        });
    }

    CommunityPrimaryKeyList primaryKeys(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String tableName)
            throws RuntimeFailure {
        validateTableObjectRequest(databaseType, databaseName, schemaName, tableName);
        return withMetadata(databaseType, metadata -> {
            Object request = metadataRequest(
                    TABLE_METADATA_REQUEST_CLASS, databaseName, schemaName, tableName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "getPrimaryKeys",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "primary keys");
            if (values.size() > MAX_KEYS) {
                throw RuntimeFailure.limit("community primary keys", MAX_KEYS);
            }
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityPrimaryKeyList.Builder keys = CommunityPrimaryKeyList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                keys.addKeys(primaryKey(value, budget));
            }
            return keys.build();
        });
    }

    void validateProgrammabilityListRequest(
            String databaseType, String databaseName, String schemaName)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireNonBlank(databaseName, MAX_SCALAR_BYTES, "database_name");
        requireUtf8(schemaName, MAX_SCALAR_BYTES, "schema_name");
        requirePlugin(databaseType);
    }

    void validateFunctionRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String functionName)
            throws RuntimeFailure {
        validateProgrammabilityDetailRequest(
                databaseType,
                databaseName,
                schemaName,
                functionName,
                "function_name");
    }

    void validateProcedureRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String procedureName)
            throws RuntimeFailure {
        validateProgrammabilityDetailRequest(
                databaseType,
                databaseName,
                schemaName,
                procedureName,
                "procedure_name");
    }

    void validateTriggerRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String triggerName)
            throws RuntimeFailure {
        validateProgrammabilityDetailRequest(
                databaseType,
                databaseName,
                schemaName,
                triggerName,
                "trigger_name");
    }

    private void validateProgrammabilityDetailRequest(
            String databaseType,
            String databaseName,
            String schemaName,
            String name,
            String nameField)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        requireUtf8(databaseName, MAX_SCALAR_BYTES, "database_name");
        requireUtf8(schemaName, MAX_SCALAR_BYTES, "schema_name");
        requireNonBlank(name, MAX_SCALAR_BYTES, nameField);
        requirePlugin(databaseType);
    }

    CommunityFunctionList functions(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName)
            throws RuntimeFailure {
        validateProgrammabilityListRequest(databaseType, databaseName, schemaName);
        return withMetadata(databaseType, metadata -> {
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "functions",
                            new Class<?>[] {Connection.class, String.class, String.class},
                            connection,
                            databaseName,
                            schemaName),
                    "functions");
            requireCount(values.size(), MAX_FUNCTIONS, "community functions");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityFunctionList.Builder functions = CommunityFunctionList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                functions.addFunctions(function(value, budget));
            }
            return functions.build();
        });
    }

    CommunityFunction function(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String functionName)
            throws RuntimeFailure {
        validateFunctionRequest(databaseType, databaseName, schemaName, functionName);
        String verifiedDatabaseName =
                verifiedProgrammabilityCatalog(databaseType, connection, databaseName);
        return withMetadata(databaseType, metadata -> {
            String lookupDatabaseName = h2SqlLiteral(
                    databaseType,
                    programmabilityDetailDatabaseName(
                            databaseType, verifiedDatabaseName, schemaName));
            String lookupFunctionName = h2SqlLiteral(databaseType, functionName);
            Object request = namedMetadataRequest(
                    FUNCTION_METADATA_REQUEST_CLASS,
                    lookupDatabaseName,
                    schemaName,
                    "setFunctionName",
                    lookupFunctionName);
            Object value = invoke(
                    metadata,
                    "function",
                    new Class<?>[] {Connection.class, request.getClass()},
                    connection,
                    request);
            requireDetail(value, "community.function_not_found", "function");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityFunction projected = function(value, budget);
            requireH2FunctionDetail(databaseType, projected);
            return restoreProgrammabilityIdentity(
                    databaseType,
                    verifiedDatabaseName,
                    schemaName,
                    functionName,
                    projected);
        });
    }

    CommunityFunctionParameterList functionParameters(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String functionName)
            throws RuntimeFailure {
        validateFunctionRequest(databaseType, databaseName, schemaName, functionName);
        return withMetadata(databaseType, metadata -> {
            Object request = namedMetadataRequest(
                    FUNCTION_METADATA_REQUEST_CLASS,
                    databaseName,
                    schemaName,
                    "setFunctionName",
                    functionName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "getFunctionParameters",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "function parameters");
            requireCount(
                    values.size(), MAX_ROUTINE_PARAMETERS, "community function parameters");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityFunctionParameterList.Builder parameters =
                    CommunityFunctionParameterList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                parameters.addParameters(functionParameter(value, budget));
            }
            return parameters.build();
        });
    }

    CommunityProcedureList procedures(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName)
            throws RuntimeFailure {
        validateProgrammabilityListRequest(databaseType, databaseName, schemaName);
        return withMetadata(databaseType, metadata -> {
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "procedures",
                            new Class<?>[] {Connection.class, String.class, String.class},
                            connection,
                            databaseName,
                            schemaName),
                    "procedures");
            requireCount(values.size(), MAX_PROCEDURES, "community procedures");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityProcedureList.Builder procedures = CommunityProcedureList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                procedures.addProcedures(procedure(value, budget));
            }
            return procedures.build();
        });
    }

    CommunityProcedure procedure(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String procedureName)
            throws RuntimeFailure {
        validateProcedureRequest(databaseType, databaseName, schemaName, procedureName);
        String verifiedDatabaseName =
                verifiedProgrammabilityCatalog(databaseType, connection, databaseName);
        return withMetadata(databaseType, metadata -> {
            String lookupDatabaseName = h2SqlLiteral(
                    databaseType,
                    programmabilityDetailDatabaseName(
                            databaseType, verifiedDatabaseName, schemaName));
            String lookupProcedureName = h2SqlLiteral(databaseType, procedureName);
            Object request = namedMetadataRequest(
                    PROCEDURE_METADATA_REQUEST_CLASS,
                    lookupDatabaseName,
                    schemaName,
                    "setProcedureName",
                    lookupProcedureName);
            Object value = invoke(
                    metadata,
                    "procedure",
                    new Class<?>[] {Connection.class, request.getClass()},
                    connection,
                    request);
            requireDetail(value, "community.procedure_not_found", "procedure");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityProcedure projected = procedure(value, budget);
            requireH2ProcedureDetail(databaseType, projected);
            return restoreProgrammabilityIdentity(
                    databaseType,
                    verifiedDatabaseName,
                    schemaName,
                    procedureName,
                    projected);
        });
    }

    CommunityProcedureParameterList procedureParameters(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String procedureName)
            throws RuntimeFailure {
        validateProcedureRequest(databaseType, databaseName, schemaName, procedureName);
        return withMetadata(databaseType, metadata -> {
            Object request = namedMetadataRequest(
                    PROCEDURE_METADATA_REQUEST_CLASS,
                    databaseName,
                    schemaName,
                    "setProcedureName",
                    procedureName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "getProcedureParameters",
                            new Class<?>[] {Connection.class, request.getClass()},
                            connection,
                            request),
                    "procedure parameters");
            requireCount(
                    values.size(), MAX_ROUTINE_PARAMETERS, "community procedure parameters");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityProcedureParameterList.Builder parameters =
                    CommunityProcedureParameterList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                parameters.addParameters(procedureParameter(value, budget));
            }
            return parameters.build();
        });
    }

    CommunityTriggerList triggers(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName)
            throws RuntimeFailure {
        validateProgrammabilityListRequest(databaseType, databaseName, schemaName);
        return withMetadata(databaseType, metadata -> {
            String lookupDatabaseName = h2SqlLiteral(databaseType, databaseName);
            String lookupSchemaName = h2SqlLiteral(databaseType, schemaName);
            List<?> values = requireList(
                    invoke(
                            metadata,
                            "triggers",
                            new Class<?>[] {Connection.class, String.class, String.class},
                            connection,
                            lookupDatabaseName,
                            lookupSchemaName),
                    "triggers");
            requireCount(values.size(), MAX_TRIGGERS, "community triggers");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityTriggerList.Builder triggers = CommunityTriggerList.newBuilder();
            for (Object value : values) {
                budget.consumeMessage();
                CommunityTrigger projected = trigger(value, budget);
                triggers.addTriggers(restoreProgrammabilityIdentity(
                        databaseType,
                        databaseName,
                        schemaName,
                        projected.getName(),
                        projected));
            }
            return triggers.build();
        });
    }

    CommunityTrigger trigger(
            String databaseType,
            Connection connection,
            String databaseName,
            String schemaName,
            String triggerName)
            throws RuntimeFailure {
        validateTriggerRequest(databaseType, databaseName, schemaName, triggerName);
        String verifiedDatabaseName =
                verifiedProgrammabilityCatalog(databaseType, connection, databaseName);
        return withMetadata(databaseType, metadata -> {
            String lookupDatabaseName = h2SqlLiteral(
                    databaseType,
                    programmabilityDetailDatabaseName(
                            databaseType, verifiedDatabaseName, schemaName));
            String lookupTriggerName = h2SqlLiteral(databaseType, triggerName);
            Object request = namedMetadataRequest(
                    TRIGGER_METADATA_REQUEST_CLASS,
                    lookupDatabaseName,
                    schemaName,
                    "setTriggerName",
                    lookupTriggerName);
            Object value = invoke(
                    metadata,
                    "trigger",
                    new Class<?>[] {Connection.class, request.getClass()},
                    connection,
                    request);
            requireDetail(value, "community.trigger_not_found", "trigger");
            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            CommunityTrigger projected = trigger(value, budget);
            requireH2TriggerDetail(databaseType, projected);
            return restoreProgrammabilityIdentity(
                    databaseType,
                    verifiedDatabaseName,
                    schemaName,
                    triggerName,
                    projected);
        });
    }

    private static String programmabilityDetailDatabaseName(
            String databaseType, String databaseName, String schemaName) {
        // Community 5.3.0 H2Meta uses request.databaseName as the schema predicate
        // for function, procedure, and trigger detail queries.
        return isH2(databaseType) ? schemaName : databaseName;
    }

    private static String verifiedProgrammabilityCatalog(
            String databaseType, Connection connection, String requestedDatabaseName)
            throws RuntimeFailure {
        if (!isH2(databaseType)) {
            return requestedDatabaseName;
        }
        String catalog;
        try {
            catalog = connection.getCatalog();
        } catch (SQLException failure) {
            throw metadataFailure(failure);
        }
        if (!requestedDatabaseName.equals(catalog)) {
            throw RuntimeFailure.validation(
                    "community.catalog_mismatch",
                    "the requested Community database does not match the active connection");
        }
        return catalog;
    }

    private static String h2SqlLiteral(String databaseType, String value) {
        return isH2(databaseType) ? value.replace("'", "''") : value;
    }

    private static void requireH2FunctionDetail(
            String databaseType, CommunityFunction projected) throws RuntimeFailure {
        if (isH2(databaseType)
                && projected.getSpecificName().isBlank()
                && projected.getBody().isBlank()) {
            requireDetail(null, "community.function_not_found", "function");
        }
    }

    private static void requireH2ProcedureDetail(
            String databaseType, CommunityProcedure projected) throws RuntimeFailure {
        if (isH2(databaseType)
                && projected.getSpecificName().isBlank()
                && projected.getBody().isBlank()) {
            requireDetail(null, "community.procedure_not_found", "procedure");
        }
    }

    private static void requireH2TriggerDetail(
            String databaseType, CommunityTrigger projected) throws RuntimeFailure {
        if (isH2(databaseType) && projected.getBody().isBlank()) {
            requireDetail(null, "community.trigger_not_found", "trigger");
        }
    }

    private static boolean isH2(String databaseType) {
        return H2_DATABASE_TYPE.equalsIgnoreCase(databaseType);
    }

    private static CommunityFunction restoreProgrammabilityIdentity(
            String databaseType,
            String databaseName,
            String schemaName,
            String functionName,
            CommunityFunction projected) {
        return isH2(databaseType)
                ? projected.toBuilder()
                        .setDatabaseName(databaseName)
                        .setSchemaName(schemaName)
                        .setName(functionName)
                        .build()
                : projected;
    }

    private static CommunityProcedure restoreProgrammabilityIdentity(
            String databaseType,
            String databaseName,
            String schemaName,
            String procedureName,
            CommunityProcedure projected) {
        return isH2(databaseType)
                ? projected.toBuilder()
                        .setDatabaseName(databaseName)
                        .setSchemaName(schemaName)
                        .setName(procedureName)
                        .build()
                : projected;
    }

    private static CommunityTrigger restoreProgrammabilityIdentity(
            String databaseType,
            String databaseName,
            String schemaName,
            String triggerName,
            CommunityTrigger projected) {
        return isH2(databaseType)
                ? projected.toBuilder()
                        .setDatabaseName(databaseName)
                        .setSchemaName(schemaName)
                        .setName(triggerName)
                        .build()
                : projected;
    }

    CommunityBuiltSql buildCreateSchema(String databaseType, CommunitySchema requested)
            throws RuntimeFailure {
        ensureOpen();
        requireDatabaseType(databaseType);
        if (requested == null) {
            throw RuntimeFailure.validation(
                    "community.schema_required", "schema is required");
        }
        try {
            CommunityBuiltNamespaceSql built = buildNamespace(
                    BuildCommunityNamespaceSqlRequest.newBuilder()
                            .setDatabaseType(databaseType)
                            .setCreateSchema(CommunityCreateSchemaSql.newBuilder()
                                    .setSchema(requested))
                            .build());
            return CommunityBuiltSql.newBuilder().setSql(built.getSql()).build();
        } catch (RuntimeFailure failure) {
            if (failure.code().equals("community.namespace_builder_not_supported")) {
                throw RuntimeFailure.validation(
                        "community.sql_builder_not_supported",
                        "the selected Community SQL builder does not support CREATE SCHEMA");
            }
            if (failure.code().equals("community.namespace_builder_failed")) {
                throw RuntimeFailure.internal(
                        "community.sql_builder_failed",
                        "the Community SQL builder failed",
                        failure.getCause());
            }
            throw failure;
        }
    }

    synchronized CommunitySqlAnalysis parse(String databaseType, String sql) throws RuntimeFailure {
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
                analysis.addStatements(parsedStatement(statement, budget));
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

    synchronized CommunitySqlValidation validate(String databaseType, String sql)
            throws RuntimeFailure {
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
            Object response = invoke(
                    parser, "parserStatements", new Class<?>[] {String.class}, sql);
            List<?> statements = requireList(invoke(response, "getStatements"), "statements");
            List<?> diagnostics =
                    requireList(invoke(response, "getSyntaxErrors"), "SQL diagnostics");
            requireCount(statements.size(), MAX_STATEMENTS, "community parsed statements");
            requireSqlDiagnosticCount(diagnostics.size());

            ProjectionBudget budget = ProjectionBudget.response();
            budget.consumeMessage();
            budget.consumeBoolean();
            CommunitySqlValidation.Builder validation =
                    CommunitySqlValidation.newBuilder().setValid(diagnostics.isEmpty());
            for (Object statement : statements) {
                budget.consumeMessage();
                validation.addStatements(parsedStatement(statement, budget));
            }
            for (Object diagnostic : diagnostics) {
                budget.consumeMessage();
                validation.addDiagnostics(sqlDiagnostic(diagnostic, budget));
            }
            return validation.build();
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

    private static CommunityParsedStatement parsedStatement(
            Object statement, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return CommunityParsedStatement.newBuilder()
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
                        budget))
                .build();
    }

    private static CommunitySqlDiagnostic sqlDiagnostic(
            Object diagnostic, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return CommunitySqlDiagnostic.newBuilder()
                .setStartLine(getNonNegativeInteger(
                        diagnostic, "getErrorStartLine", "diagnostic_start_line", budget))
                .setStartColumn(getNonNegativeInteger(
                        diagnostic,
                        "getErrorStartPositionInLine",
                        "diagnostic_start_column",
                        budget))
                .setEndLine(getNonNegativeInteger(
                        diagnostic, "getErrorEndLine", "diagnostic_end_line", budget))
                .setEndColumn(getNonNegativeInteger(
                        diagnostic,
                        "getErrorEndPositionInLine",
                        "diagnostic_end_column",
                        budget))
                .setTokenText(getString(
                        diagnostic,
                        "getErrorTokenText",
                        MAX_SQL_BYTES,
                        "diagnostic_token_text",
                        budget))
                .setMessage(getString(
                        diagnostic,
                        "getErrorMessage",
                        MAX_COMMENT_BYTES,
                        "diagnostic_message",
                        budget))
                .build();
    }

    static void requireSqlDiagnosticCount(int count) throws RuntimeFailure {
        requireCount(count, MAX_SQL_DIAGNOSTICS, "community SQL diagnostics");
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
            Object builder = optionalComponent(handle.plugin(), "getSqlBuilder");
            Object valueProcessor = optionalComponent(handle.plugin(), "getValueProcessor");
            Object identifierProcessor =
                    optionalComponent(handle.plugin(), "getSQLIdentifierProcessor");
            Object syntax = invoke(handle.plugin(), "getSqlSyntaxPlugin");
            budget.consumeBooleans(6);
            descriptor.setMetadataAvailable(metadata != null);
            descriptor.setSqlBuilderAvailable(builder != null);
            descriptor.setSqlParserAvailable(syntax != null && invoke(syntax, "getSQLParser") != null);
            descriptor.setDmlBuilderAvailable(dmlSegmentAvailable(builder));
            descriptor.setValueProcessorAvailable(valueProcessor != null);
            descriptor.setIdentifierProcessorAvailable(identifierProcessor != null);
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

    private CommunityDatabase database(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityDatabase.Builder database = CommunityDatabase.newBuilder()
                .setName(getString(
                        value, "getName", MAX_SCALAR_BYTES, "database_name", budget))
                .setComment(getString(
                        value, "getComment", MAX_COMMENT_BYTES, "database_comment", budget))
                .setCharset(getString(
                        value, "getCharset", MAX_SCALAR_BYTES, "database_charset", budget))
                .setCollation(getString(
                        value, "getCollation", MAX_SCALAR_BYTES, "database_collation", budget))
                .setOwner(getString(
                        value, "getOwner", MAX_SCALAR_BYTES, "database_owner", budget))
                .setSystem(getBoolean(value, "isSystem"));
        budget.consumeBoolean();
        return database.build();
    }

    private CommunityTable table(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityTable.Builder table = CommunityTable.newBuilder()
                .setDatabaseName(getString(
                        value, "getDatabaseName", MAX_SCALAR_BYTES, "table_database_name", budget))
                .setSchemaName(getString(
                        value, "getSchemaName", MAX_SCALAR_BYTES, "table_schema_name", budget))
                .setName(getString(
                        value, "getName", MAX_SCALAR_BYTES, "table_name", budget))
                .setType(getString(
                        value, "getType", MAX_SCALAR_BYTES, "table_type", budget))
                .setComment(getString(
                        value, "getComment", MAX_COMMENT_BYTES, "table_comment", budget))
                .setDatabaseType(getString(
                        value, "getDbType", MAX_DATABASE_TYPE_BYTES, "table_database_type", budget))
                .setPinned(getBoolean(value, "isPinned"))
                .setDdl(getString(value, "getDdl", MAX_SQL_BYTES, "table_ddl", budget))
                .setEngine(getString(
                        value, "getEngine", MAX_SCALAR_BYTES, "table_engine", budget))
                .setCharset(getString(
                        value, "getCharset", MAX_SCALAR_BYTES, "table_charset", budget))
                .setCollation(getString(
                        value, "getCollate", MAX_SCALAR_BYTES, "table_collation", budget))
                .setPartition(getString(
                        value, "getPartition", MAX_SQL_BYTES, "table_partition", budget))
                .setTablespace(getString(
                        value, "getTablespace", MAX_SCALAR_BYTES, "table_tablespace", budget))
                .setCreateTime(getString(
                        value, "getCreateTime", MAX_SCALAR_BYTES, "table_create_time", budget))
                .setUpdateTime(getString(
                        value, "getUpdateTime", MAX_SCALAR_BYTES, "table_update_time", budget));
        budget.consumeBoolean();
        setOptionalLong(table::setIncrementValue, value, "getIncrementValue", budget);
        setOptionalLong(table::setRows, value, "getRows", budget);
        setOptionalLong(table::setDataLength, value, "getDataLength", budget);
        return table.build();
    }

    private CommunityForeignKey foreignKey(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return CommunityForeignKey.newBuilder()
                .setPrimaryTableDatabase(getString(
                        value,
                        "getPkTableCat",
                        MAX_SCALAR_BYTES,
                        "foreign_key_primary_database",
                        budget))
                .setPrimaryTableSchema(getString(
                        value,
                        "getPkTableSchem",
                        MAX_SCALAR_BYTES,
                        "foreign_key_primary_schema",
                        budget))
                .setPrimaryTableName(getString(
                        value,
                        "getPkTableName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_primary_table",
                        budget))
                .setPrimaryColumnName(getString(
                        value,
                        "getPkColumnName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_primary_column",
                        budget))
                .setForeignTableDatabase(getString(
                        value,
                        "getFkTableCat",
                        MAX_SCALAR_BYTES,
                        "foreign_key_foreign_database",
                        budget))
                .setForeignTableSchema(getString(
                        value,
                        "getFkTableSchem",
                        MAX_SCALAR_BYTES,
                        "foreign_key_foreign_schema",
                        budget))
                .setForeignTableName(getString(
                        value,
                        "getFkTableName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_foreign_table",
                        budget))
                .setForeignColumnName(getString(
                        value,
                        "getFkColumnName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_foreign_column",
                        budget))
                .setKeySequence(getRequiredInteger(value, "getKeySeq", budget))
                .setUpdateRule(getRequiredInteger(value, "getUpdateRule", budget))
                .setDeleteRule(getRequiredInteger(value, "getDeleteRule", budget))
                .setForeignKeyName(getString(
                        value,
                        "getFkName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_name",
                        budget))
                .setPrimaryKeyName(getString(
                        value,
                        "getPkName",
                        MAX_SCALAR_BYTES,
                        "foreign_key_primary_key_name",
                        budget))
                .setDeferrability(getRequiredInteger(value, "getDeferrability", budget))
                .build();
    }

    private CommunityPrimaryKey primaryKey(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return CommunityPrimaryKey.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "primary_key_database",
                        budget))
                .setSchemaName(getString(
                        value,
                        "getSchemaName",
                        MAX_SCALAR_BYTES,
                        "primary_key_schema",
                        budget))
                .setTableName(getString(
                        value,
                        "getTableName",
                        MAX_SCALAR_BYTES,
                        "primary_key_table",
                        budget))
                .setColumnName(getString(
                        value,
                        "getColumnName",
                        MAX_SCALAR_BYTES,
                        "primary_key_column",
                        budget))
                .setName(getString(
                        value,
                        "getPrimaryKeyName",
                        MAX_SCALAR_BYTES,
                        "primary_key_name",
                        budget))
                .build();
    }

    private CommunityFunction function(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityFunction.Builder function = CommunityFunction.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "function_database_name",
                        budget))
                .setSchemaName(getString(
                        value,
                        "getSchemaName",
                        MAX_SCALAR_BYTES,
                        "function_schema_name",
                        budget))
                .setName(getRequiredString(
                        value,
                        "getFunctionName",
                        MAX_SCALAR_BYTES,
                        "function_name",
                        budget))
                .setRemarks(getString(
                        value, "getRemarks", MAX_COMMENT_BYTES, "function_remarks", budget))
                .setSpecificName(getString(
                        value,
                        "getSpecificName",
                        MAX_SCALAR_BYTES,
                        "function_specific_name",
                        budget))
                .setBody(getString(
                        value, "getFunctionBody", MAX_SQL_BYTES, "function_body", budget))
                .setTemplate(getString(
                        value,
                        "getFunctionTemplate",
                        MAX_SQL_BYTES,
                        "function_template",
                        budget));
        setOptionalInteger(function::setFunctionType, value, "getFunctionType", budget);
        return function.build();
    }

    private CommunityFunctionParameter functionParameter(
            Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityFunctionParameter.Builder parameter = CommunityFunctionParameter.newBuilder()
                .setFunctionDatabase(getString(
                        value,
                        "getFunctionCat",
                        MAX_SCALAR_BYTES,
                        "function_parameter_database",
                        budget))
                .setFunctionSchema(getString(
                        value,
                        "getFunctionSchem",
                        MAX_SCALAR_BYTES,
                        "function_parameter_schema",
                        budget))
                .setFunctionName(getRequiredString(
                        value,
                        "getFunctionName",
                        MAX_SCALAR_BYTES,
                        "function_parameter_function_name",
                        budget))
                .setColumnName(getString(
                        value,
                        "getColumnName",
                        MAX_SCALAR_BYTES,
                        "function_parameter_column_name",
                        budget))
                .setTypeName(getString(
                        value,
                        "getTypeName",
                        MAX_SCALAR_BYTES,
                        "function_parameter_type_name",
                        budget))
                .setRemarks(getString(
                        value,
                        "getRemarks",
                        MAX_COMMENT_BYTES,
                        "function_parameter_remarks",
                        budget))
                .setIsNullable(getString(
                        value,
                        "getIsNullable",
                        MAX_SCALAR_BYTES,
                        "function_parameter_is_nullable",
                        budget))
                .setSpecificName(getString(
                        value,
                        "getSpecificName",
                        MAX_SCALAR_BYTES,
                        "function_parameter_specific_name",
                        budget));
        setOptionalInteger(parameter::setColumnType, value, "getColumnType", budget);
        setOptionalInteger(parameter::setDataType, value, "getDataType", budget);
        setOptionalInteger(parameter::setPrecision, value, "getPrecision", budget);
        setOptionalInteger(parameter::setLength, value, "getLength", budget);
        setOptionalInteger(parameter::setScale, value, "getScale", budget);
        setOptionalInteger(parameter::setRadix, value, "getRadix", budget);
        setOptionalInteger(parameter::setNullable, value, "getNullable", budget);
        setOptionalInteger(parameter::setCharOctetLength, value, "getCharOctetLength", budget);
        setOptionalInteger(parameter::setOrdinalPosition, value, "getOrdinalPosition", budget);
        return parameter.build();
    }

    private CommunityProcedure procedure(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityProcedure.Builder procedure = CommunityProcedure.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "procedure_database_name",
                        budget))
                .setSchemaName(getString(
                        value,
                        "getSchemaName",
                        MAX_SCALAR_BYTES,
                        "procedure_schema_name",
                        budget))
                .setName(getRequiredString(
                        value,
                        "getProcedureName",
                        MAX_SCALAR_BYTES,
                        "procedure_name",
                        budget))
                .setRemarks(getString(
                        value, "getRemarks", MAX_COMMENT_BYTES, "procedure_remarks", budget))
                .setSpecificName(getString(
                        value,
                        "getSpecificName",
                        MAX_SCALAR_BYTES,
                        "procedure_specific_name",
                        budget))
                .setBody(getString(
                        value, "getProcedureBody", MAX_SQL_BYTES, "procedure_body", budget));
        setOptionalInteger(procedure::setProcedureType, value, "getProcedureType", budget);
        return procedure.build();
    }

    private CommunityProcedureParameter procedureParameter(
            Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityProcedureParameter.Builder parameter = CommunityProcedureParameter.newBuilder()
                .setProcedureDatabase(getString(
                        value,
                        "getProcedureCat",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_database",
                        budget))
                .setProcedureSchema(getString(
                        value,
                        "getProcedureSchem",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_schema",
                        budget))
                .setProcedureName(getRequiredString(
                        value,
                        "getProcedureName",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_procedure_name",
                        budget))
                .setColumnName(getString(
                        value,
                        "getColumnName",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_column_name",
                        budget))
                .setTypeName(getString(
                        value,
                        "getTypeName",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_type_name",
                        budget))
                .setRemarks(getString(
                        value,
                        "getRemarks",
                        MAX_COMMENT_BYTES,
                        "procedure_parameter_remarks",
                        budget))
                .setColumnDefault(getString(
                        value,
                        "getColumnDef",
                        MAX_SQL_BYTES,
                        "procedure_parameter_column_default",
                        budget))
                .setIsNullable(getString(
                        value,
                        "getIsNullable",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_is_nullable",
                        budget))
                .setSpecificName(getString(
                        value,
                        "getSpecificName",
                        MAX_SCALAR_BYTES,
                        "procedure_parameter_specific_name",
                        budget));
        setOptionalInteger(parameter::setColumnType, value, "getColumnType", budget);
        setOptionalInteger(parameter::setDataType, value, "getDataType", budget);
        setOptionalInteger(parameter::setPrecision, value, "getPrecision", budget);
        setOptionalInteger(parameter::setLength, value, "getLength", budget);
        setOptionalInteger(parameter::setScale, value, "getScale", budget);
        setOptionalInteger(parameter::setRadix, value, "getRadix", budget);
        setOptionalInteger(parameter::setNullable, value, "getNullable", budget);
        setOptionalInteger(parameter::setSqlDataType, value, "getSqlDataType", budget);
        setOptionalInteger(parameter::setSqlDatetimeSub, value, "getSqlDatetimeSub", budget);
        setOptionalInteger(parameter::setCharOctetLength, value, "getCharOctetLength", budget);
        setOptionalInteger(parameter::setOrdinalPosition, value, "getOrdinalPosition", budget);
        return parameter.build();
    }

    private CommunityTrigger trigger(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return CommunityTrigger.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "trigger_database_name",
                        budget))
                .setSchemaName(getString(
                        value,
                        "getSchemaName",
                        MAX_SCALAR_BYTES,
                        "trigger_schema_name",
                        budget))
                .setName(getRequiredString(
                        value,
                        "getTriggerName",
                        MAX_SCALAR_BYTES,
                        "trigger_name",
                        budget))
                .setEventManipulation(getString(
                        value,
                        "getEventManipulation",
                        MAX_SCALAR_BYTES,
                        "trigger_event_manipulation",
                        budget))
                .setBody(getString(
                        value, "getTriggerBody", MAX_SQL_BYTES, "trigger_body", budget))
                .build();
    }

    private CommunityTableColumn column(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityTableColumn.Builder column = CommunityTableColumn.newBuilder()
                .setDatabaseName(getString(
                        value, "getDatabaseName", MAX_SCALAR_BYTES, "column_database_name", budget))
                .setSchemaName(getString(
                        value, "getSchemaName", MAX_SCALAR_BYTES, "column_schema_name", budget))
                .setTableName(getString(
                        value, "getTableName", MAX_SCALAR_BYTES, "column_table_name", budget))
                .setName(getString(
                        value, "getName", MAX_SCALAR_BYTES, "column_name", budget))
                .setColumnType(getString(
                        value, "getColumnType", MAX_SCALAR_BYTES, "column_type", budget))
                .setDefaultValue(getString(
                        value, "getDefaultValue", MAX_SQL_BYTES, "column_default_value", budget))
                .setComment(getString(
                        value, "getComment", MAX_COMMENT_BYTES, "column_comment", budget))
                .setPrimaryKeyName(getString(
                        value,
                        "getPrimaryKeyName",
                        MAX_SCALAR_BYTES,
                        "column_primary_key_name",
                        budget))
                .setPrimaryKeyOrder(getRequiredInteger(value, "getPrimaryKeyOrder", budget))
                .setExtent(getString(
                        value, "getExtent", MAX_SCALAR_BYTES, "column_extent", budget))
                .setCharset(getString(
                        value, "getCharSetName", MAX_SCALAR_BYTES, "column_charset", budget))
                .setCollation(getString(
                        value, "getCollationName", MAX_SCALAR_BYTES, "column_collation", budget))
                .setUnit(getString(
                        value, "getUnit", MAX_SCALAR_BYTES, "column_unit", budget))
                .setDefaultConstraintName(getString(
                        value,
                        "getDefaultConstraintName",
                        MAX_SCALAR_BYTES,
                        "column_default_constraint_name",
                        budget));
        setOptionalInteger(column::setDataType, value, "getDataType", budget);
        setOptionalBoolean(column::setAutoIncrement, value, "getAutoIncrement", budget);
        setOptionalBoolean(column::setPrimaryKey, value, "getPrimaryKey", budget);
        setOptionalInteger(column::setColumnSize, value, "getColumnSize", budget);
        setOptionalInteger(column::setBufferLength, value, "getBufferLength", budget);
        setOptionalInteger(column::setDecimalDigits, value, "getDecimalDigits", budget);
        setOptionalInteger(column::setNumPrecRadix, value, "getNumPrecRadix", budget);
        setOptionalInteger(column::setSqlDataType, value, "getSqlDataType", budget);
        setOptionalInteger(column::setSqlDatetimeSub, value, "getSqlDatetimeSub", budget);
        setOptionalInteger(column::setCharOctetLength, value, "getCharOctetLength", budget);
        setOptionalInteger(column::setOrdinalPosition, value, "getOrdinalPosition", budget);
        setOptionalInteger(column::setNullable, value, "getNullable", budget);
        setOptionalBoolean(column::setGeneratedColumn, value, "getGeneratedColumn", budget);
        setOptionalBoolean(column::setSparse, value, "getSparse", budget);
        setOptionalInteger(column::setSeed, value, "getSeed", budget);
        setOptionalInteger(column::setIncrement, value, "getIncrement", budget);
        setOptionalBoolean(
                column::setOnUpdateCurrentTimestamp,
                value,
                "getOnUpdateCurrentTimestamp",
                budget);
        return column.build();
    }

    private CommunityTableIndex index(
            Object value, ProjectionBudget budget, int[] projectedColumns)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityTableIndex.Builder index = CommunityTableIndex.newBuilder()
                .setDatabaseName(getString(
                        value, "getDatabaseName", MAX_SCALAR_BYTES, "index_database_name", budget))
                .setSchemaName(getString(
                        value, "getSchemaName", MAX_SCALAR_BYTES, "index_schema_name", budget))
                .setTableName(getString(
                        value, "getTableName", MAX_SCALAR_BYTES, "index_table_name", budget))
                .setName(getString(
                        value, "getName", MAX_SCALAR_BYTES, "index_name", budget))
                .setType(getString(
                        value, "getType", MAX_SCALAR_BYTES, "index_type", budget))
                .setComment(getString(
                        value, "getComment", MAX_COMMENT_BYTES, "index_comment", budget))
                .setMethod(getString(
                        value, "getMethod", MAX_SCALAR_BYTES, "index_method", budget))
                .setForeignSchemaName(getString(
                        value,
                        "getForeignSchemaName",
                        MAX_SCALAR_BYTES,
                        "index_foreign_schema_name",
                        budget))
                .setForeignTableName(getString(
                        value,
                        "getForeignTableName",
                        MAX_SCALAR_BYTES,
                        "index_foreign_table_name",
                        budget));
        setOptionalBoolean(index::setUnique, value, "getUnique", budget);
        setOptionalBoolean(index::setConcurrently, value, "getConcurrently", budget);
        for (Object rawColumn : requireList(invoke(value, "getColumnList"), "index columns")) {
            requireIndexColumnCapacity(projectedColumns);
            budget.consumeMessage();
            index.addColumns(indexColumn(rawColumn, budget));
        }
        for (Object rawName : requireList(
                invoke(value, "getForeignColumnNamelist"), "foreign index columns")) {
            requireIndexColumnCapacity(projectedColumns);
            index.addForeignColumnNames(projectString(
                    scalar(rawName),
                    MAX_SCALAR_BYTES,
                    "index_foreign_column_name",
                    budget));
        }
        return index.build();
    }

    private CommunityTableIndexColumn indexColumn(Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunityTableIndexColumn.Builder column = CommunityTableIndexColumn.newBuilder()
                .setDatabaseName(getString(
                        value,
                        "getDatabaseName",
                        MAX_SCALAR_BYTES,
                        "index_column_database_name",
                        budget))
                .setSchemaName(getString(
                        value,
                        "getSchemaName",
                        MAX_SCALAR_BYTES,
                        "index_column_schema_name",
                        budget))
                .setTableName(getString(
                        value,
                        "getTableName",
                        MAX_SCALAR_BYTES,
                        "index_column_table_name",
                        budget))
                .setIndexName(getString(
                        value,
                        "getIndexName",
                        MAX_SCALAR_BYTES,
                        "index_column_index_name",
                        budget))
                .setColumnName(getString(
                        value,
                        "getColumnName",
                        MAX_SCALAR_BYTES,
                        "index_column_name",
                        budget))
                .setType(getString(
                        value, "getType", MAX_SCALAR_BYTES, "index_column_type", budget))
                .setComment(getString(
                        value,
                        "getComment",
                        MAX_COMMENT_BYTES,
                        "index_column_comment",
                        budget))
                .setCollation(getString(
                        value,
                        "getCollation",
                        MAX_SCALAR_BYTES,
                        "index_column_collation",
                        budget))
                .setIndexQualifier(getString(
                        value,
                        "getIndexQualifier",
                        MAX_SCALAR_BYTES,
                        "index_column_qualifier",
                        budget))
                .setSortOrder(getString(
                        value,
                        "getAscOrDesc",
                        MAX_SCALAR_BYTES,
                        "index_column_sort_order",
                        budget))
                .setFilterCondition(getString(
                        value,
                        "getFilterCondition",
                        MAX_SQL_BYTES,
                        "index_column_filter_condition",
                        budget));
        setOptionalInteger(column::setOrdinalPosition, value, "getOrdinalPosition", budget);
        setOptionalBoolean(column::setNonUnique, value, "getNonUnique", budget);
        setOptionalLong(column::setCardinality, value, "getCardinality", budget);
        setOptionalLong(column::setPages, value, "getPages", budget);
        setOptionalLong(column::setSubPart, value, "getSubPart", budget);
        return column.build();
    }

    private void requireIndexColumnCapacity(int[] projectedColumns) throws RuntimeFailure {
        projectedColumns[0]++;
        if (projectedColumns[0] > MAX_INDEX_COLUMNS) {
            throw RuntimeFailure.limit("community index columns", MAX_INDEX_COLUMNS);
        }
    }

    private Object metadataRequest(
            String className, String databaseName, String schemaName, String tableName)
            throws ReflectiveOperationException {
        Class<?> requestType = Class.forName(className, true, loader);
        Object request = requestType.getDeclaredConstructor().newInstance();
        invokeSetter(request, "setDatabaseName", String.class, databaseName);
        invokeSetter(request, "setSchemaName", String.class, schemaName);
        invokeSetter(request, "setTableName", String.class, tableName);
        return request;
    }

    private Object viewMetadataRequest(
            String databaseName, String schemaName, String viewName)
            throws ReflectiveOperationException {
        Class<?> requestType = Class.forName(VIEW_METADATA_REQUEST_CLASS, true, loader);
        Object request = requestType.getDeclaredConstructor().newInstance();
        invokeSetter(request, "setDatabaseName", String.class, databaseName);
        invokeSetter(request, "setSchemaName", String.class, schemaName);
        invokeSetter(request, "setViewName", String.class, viewName);
        return request;
    }

    private Object namedMetadataRequest(
            String className,
            String databaseName,
            String schemaName,
            String nameSetter,
            String name)
            throws ReflectiveOperationException {
        Class<?> requestType = Class.forName(className, true, loader);
        Object request = requestType.getDeclaredConstructor().newInstance();
        invokeSetter(request, "setDatabaseName", String.class, databaseName);
        invokeSetter(request, "setSchemaName", String.class, schemaName);
        invokeSetter(request, nameSetter, String.class, name);
        return request;
    }

    private <T> T withMetadata(String databaseType, MetadataInvocation<T> invocation)
            throws RuntimeFailure {
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
            return invocation.invoke(metadata);
        } catch (RuntimeFailure failure) {
            throw failure;
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            throw metadataFailure(failure);
        } finally {
            thread.setContextClassLoader(previous);
        }
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

    private static boolean dmlSegmentAvailable(Object builder)
            throws ReflectiveOperationException {
        if (builder == null) {
            return false;
        }
        try {
            return invoke(builder, "dml") != null;
        } catch (InvocationTargetException failure) {
            if (rootInvocationCause(failure) instanceof UnsupportedOperationException) {
                return false;
            }
            throw failure;
        }
    }

    private static Object optionalComponent(Object plugin, String method)
            throws ReflectiveOperationException {
        try {
            return invoke(plugin, method);
        } catch (InvocationTargetException failure) {
            if (rootInvocationCause(failure) instanceof UnsupportedOperationException) {
                return null;
            }
            throw failure;
        }
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

    private static String getRequiredString(
            Object target,
            String getter,
            int maximum,
            String field,
            ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        String value = getString(target, getter, maximum, field, budget);
        requireNonBlank(value, maximum, field);
        return value;
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

    private static int getRequiredInteger(
            Object target, String getter, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        if (!(value instanceof Number number)) {
            throw new IllegalStateException("Community plugin returned a non-numeric value");
        }
        budget.consumeNumeric();
        return number.intValue();
    }

    private static int getNonNegativeInteger(
            Object target, String getter, String field, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return requireNonNegativeCoordinate(getRequiredInteger(target, getter, budget), field);
    }

    static int requireNonNegativeCoordinate(int value, String field) {
        if (value < 0) {
            throw new IllegalStateException(
                    "Community SQL parser returned a negative " + field);
        }
        return value;
    }

    private static void setOptionalBoolean(
            BooleanSetter setter,
            Object target,
            String getter,
            ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        if (value == null) {
            return;
        }
        setter.set(booleanValue(value));
        budget.consumeBoolean();
    }

    private static void setOptionalInteger(
            IntegerSetter setter,
            Object target,
            String getter,
            ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        if (value == null) {
            return;
        }
        if (!(value instanceof Number number)) {
            throw new IllegalStateException("Community plugin returned a non-numeric value");
        }
        setter.set(number.intValue());
        budget.consumeNumeric();
    }

    private static void setOptionalLong(
            LongSetter setter,
            Object target,
            String getter,
            ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        if (value == null) {
            return;
        }
        if (!(value instanceof Number number)) {
            throw new IllegalStateException("Community plugin returned a non-numeric value");
        }
        setter.set(number.longValue());
        budget.consumeNumeric();
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

    private static void requireCount(int count, int maximum, String field)
            throws RuntimeFailure {
        if (count > maximum) {
            throw RuntimeFailure.limit(field, maximum);
        }
    }

    static <T> T requireDetail(T value, String code, String field) throws RuntimeFailure {
        if (value == null) {
            throw RuntimeFailure.validation(
                    code, "the requested Community " + field + " was not found");
        }
        return value;
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

        void consumeNumeric() throws RuntimeFailure {
            consume(NUMERIC_FIELD_OVERHEAD_BYTES);
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

    @FunctionalInterface
    private interface MetadataInvocation<T> {
        T invoke(Object metadata) throws ReflectiveOperationException, RuntimeFailure;
    }

    @FunctionalInterface
    private interface BooleanSetter {
        void set(boolean value);
    }

    @FunctionalInterface
    private interface IntegerSetter {
        void set(int value);
    }

    @FunctionalInterface
    private interface LongSetter {
        void set(long value);
    }

    private record PluginHandle(String databaseType, Object plugin, Object config) {}
}

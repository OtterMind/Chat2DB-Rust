package ai.chat2db.rust.compat;

import ai.chat2db.rust.compat.protocol.v1.CommunityByteLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletion;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionCandidate;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionCandidateCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHint;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHintCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHintItem;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionEditorHintItemCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionPrefixLimit;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionRange;
import ai.chat2db.rust.compat.protocol.v1.CommunitySqlCompletionSnippetSlotCountLimit;
import ai.chat2db.rust.compat.protocol.v1.CompleteCommunitySqlRequest;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.sql.SQLException;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Reflective adapter around Community's real domain-core SQL completion service. */
final class CommunitySqlCompletionBridge {

    private static final String TABLE_SERVICE_CLASS =
            "ai.chat2db.community.domain.api.service.db.IDbTableService";
    private static final String GENERIC_ENGINE_CLASS =
            "ai.chat2db.community.domain.core.impl.db.GenericSqlCompletionEngine";
    private static final String CONVERTER_CLASS =
            "ai.chat2db.community.domain.core.converter.SqlCompletionConverter";
    private static final String CONVERTER_IMPL_CLASS =
            "ai.chat2db.community.domain.core.converter.SqlCompletionConverterImpl";
    private static final String COMPLETION_SERVICE_CLASS =
            "ai.chat2db.community.domain.core.impl.db.DbSqlCompletionServiceImpl";
    private static final String CONNECT_INFO_CLASS = "ai.chat2db.spi.model.datasource.ConnectInfo";
    private static final String CONTEXT_CLASS = "ai.chat2db.spi.sql.Chat2DBContext";
    private static final String COMPLETION_REQUEST_CLASS =
            "ai.chat2db.community.domain.api.model.request.sql.DbSqlCompletionGetRequest";
    private static final String ACTIVE_SNIPPET_SLOT_CLASS =
            "ai.chat2db.community.domain.api.model.completion.SqlCompletionActiveSnippetSlot";
    private static final String MEMORY_CACHE_CLASS =
            "ai.chat2db.community.domain.core.cache.MemoryCacheManage";
    private static final String COLUMN_METADATA_REQUEST_CLASS =
            "ai.chat2db.spi.model.request.ColumnMetadataRequest";
    private static final String DB_METADATA_CLASS = "ai.chat2db.spi.IDbMetaData";

    private static final int MAX_CANDIDATES = CommunitySqlCompletionCandidateCountLimit
            .COMMUNITY_SQL_COMPLETION_CANDIDATE_COUNT_LIMIT_MAX_CANDIDATES
            .getNumber();
    private static final int MAX_EDITOR_HINTS = CommunitySqlCompletionEditorHintCountLimit
            .COMMUNITY_SQL_COMPLETION_EDITOR_HINT_COUNT_LIMIT_MAX_EDITOR_HINTS
            .getNumber();
    private static final int MAX_EDITOR_HINT_ITEMS = CommunitySqlCompletionEditorHintItemCountLimit
            .COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEM_COUNT_LIMIT_MAX_EDITOR_HINT_ITEMS
            .getNumber();
    private static final int MAX_SNIPPET_SLOTS = CommunitySqlCompletionSnippetSlotCountLimit
            .COMMUNITY_SQL_COMPLETION_SNIPPET_SLOT_COUNT_LIMIT_MAX_SNIPPET_SLOTS
            .getNumber();
    private static final int MAX_MIN_PREFIX_LENGTH = CommunitySqlCompletionPrefixLimit
            .COMMUNITY_SQL_COMPLETION_PREFIX_LIMIT_MAX_MIN_PREFIX_LENGTH
            .getNumber();
    private static final int MAX_RESPONSE_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_RESPONSE_BYTES.getNumber();
    private static final int MAX_COMMENT_BYTES =
            CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_COMMENT_BYTES.getNumber();

    private final ClassLoader loader;
    private final Class<?> connectInfoType;
    private final Class<?> contextType;
    private final Class<?> requestType;
    private final Class<?> activeSnippetSlotType;
    private final Object service;

    private CommunitySqlCompletionBridge(
            ClassLoader loader,
            Class<?> connectInfoType,
            Class<?> contextType,
            Class<?> requestType,
            Class<?> activeSnippetSlotType,
            Object service) {
        this.loader = loader;
        this.connectInfoType = connectInfoType;
        this.contextType = contextType;
        this.requestType = requestType;
        this.activeSnippetSlotType = activeSnippetSlotType;
        this.service = service;
    }

    static CommunitySqlCompletionBridge open(ClassLoader loader) throws ReflectiveOperationException {
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        thread.setContextClassLoader(loader);
        try {
            Class<?> tableServiceType = Class.forName(TABLE_SERVICE_CLASS, true, loader);
            Object tableService = Proxy.newProxyInstance(
                    loader,
                    new Class<?>[] {tableServiceType},
                    metadataTableService(loader));
            Object genericEngine = construct(
                    loader,
                    GENERIC_ENGINE_CLASS,
                    new Class<?>[] {tableServiceType},
                    tableService);
            Class<?> converterType = Class.forName(CONVERTER_CLASS, true, loader);
            Object converter = construct(loader, CONVERTER_IMPL_CLASS, new Class<?>[0]);
            Object service = construct(
                    loader,
                    COMPLETION_SERVICE_CLASS,
                    new Class<?>[] {converterType, genericEngine.getClass()},
                    converter,
                    genericEngine);
            return new CommunitySqlCompletionBridge(
                    loader,
                    Class.forName(CONNECT_INFO_CLASS, true, loader),
                    Class.forName(CONTEXT_CLASS, true, loader),
                    Class.forName(COMPLETION_REQUEST_CLASS, true, loader),
                    Class.forName(ACTIVE_SNIPPET_SLOT_CLASS, true, loader),
                    service);
        } finally {
            thread.setContextClassLoader(previous);
        }
    }

    void validateRequest(CompleteCommunitySqlRequest request) throws RuntimeFailure {
        if (request == null) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_request_required",
                    "the Community SQL-completion request is required");
        }
        ProtocolLimits.requireNonBlankUtf8(
                request.getDatabaseType(),
                CommunityByteLimit.COMMUNITY_BYTE_LIMIT_MAX_DATABASE_TYPE_BYTES.getNumber(),
                "database_type");
        ProtocolLimits.requireUtf8(
                request.getDatabaseName(), ProtocolLimits.MAX_SCALAR_BYTES, "database_name");
        ProtocolLimits.requireUtf8(
                request.getSchemaName(), ProtocolLimits.MAX_SCALAR_BYTES, "schema_name");
        ProtocolLimits.requireUtf8(
                request.getDatasourceName(), ProtocolLimits.MAX_SCALAR_BYTES, "datasource_name");
        ProtocolLimits.requireNonBlankUtf8(request.getSql(), ProtocolLimits.MAX_SQL_BYTES, "sql");
        ProtocolLimits.requireNonBlankUtf8(
                request.getKeywordCase(), ProtocolLimits.MAX_SCALAR_BYTES, "keyword_case");
        String keywordCase = request.getKeywordCase().trim().toUpperCase(Locale.ROOT);
        if (!keywordCase.equals("UPPER") && !keywordCase.equals("LOWER")) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_keyword_case_invalid",
                    "keyword_case must be UPPER or LOWER");
        }
        requireDatasourceScope(request.getDatasourceScope());
        requireUtf16Offset(
                request.getCursorUtf16(), request.getSql().length(), "cursor_utf16");
        if (request.getMinPrefixLength() < 0
                || request.getMinPrefixLength() > MAX_MIN_PREFIX_LENGTH) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_min_prefix_length_invalid",
                    "min_prefix_length must be between 0 and " + MAX_MIN_PREFIX_LENGTH);
        }
        if (request.hasActiveSnippetSlot()) {
            var slot = request.getActiveSnippetSlot();
            ProtocolLimits.requireNonBlankUtf8(
                    slot.getType(), ProtocolLimits.MAX_SCALAR_BYTES, "active_snippet_slot_type");
            String slotType = slot.getType().trim().toUpperCase(Locale.ROOT);
            if (!slotType.equals("SELECT_FUNCTION")
                    && !slotType.equals("CALL_PROCEDURE")
                    && !slotType.equals("INSERT_COLUMN_LIST")) {
                throw RuntimeFailure.validation(
                        "community.sql_completion_active_snippet_slot_invalid",
                        "active snippet slot type is invalid");
            }
            requireUtf16Range(
                    slot.getReplaceStartUtf16(),
                    slot.getReplaceEndUtf16(),
                    request.getSql().length(),
                    "active snippet slot");
        }
    }

    CommunitySqlCompletion complete(
            String canonicalDatabaseType,
            Connection connection,
            CompleteCommunitySqlRequest request)
            throws RuntimeFailure {
        validateRequest(request);
        Thread thread = Thread.currentThread();
        ClassLoader previous = thread.getContextClassLoader();
        Object connectInfo = null;
        RuntimeFailure operationFailure = null;
        thread.setContextClassLoader(loader);
        try {
            connectInfo = connectInfo(canonicalDatabaseType, connection, request);
            contextType.getMethod("putContext", connectInfoType).invoke(null, connectInfo);
            Object response = service.getClass().getMethod("complete", requestType)
                    .invoke(service, completionRequest(request));
            if (response == null) {
                throw RuntimeFailure.internal(
                        "community.sql_completion_failed",
                        "the Community SQL-completion service returned no response",
                        null);
            }
            return project(response, request.getSql());
        } catch (RuntimeFailure failure) {
            operationFailure = failure;
            throw failure;
        } catch (InvocationTargetException failure) {
            RuntimeFailure translated = RuntimeFailure.internal(
                    "community.sql_completion_failed",
                    "the Community SQL-completion service failed internally",
                    rootInvocationCause(failure));
            operationFailure = translated;
            throw translated;
        } catch (ReflectiveOperationException | RuntimeException | LinkageError failure) {
            RuntimeFailure translated = RuntimeFailure.internal(
                    "community.sql_completion_failed",
                    "the Community SQL-completion service failed internally",
                    failure);
            operationFailure = translated;
            throw translated;
        } finally {
            RuntimeFailure connectionFailure = connectionOwnershipFailure(connection);
            Throwable cleanupFailure = clearCompletionState(request.getDatasourceScope());
            thread.setContextClassLoader(previous);
            if (connectionFailure != null) {
                if (operationFailure != null) {
                    connectionFailure.addSuppressed(operationFailure);
                }
                if (cleanupFailure != null) {
                    connectionFailure.addSuppressed(cleanupFailure);
                }
                throw connectionFailure;
            }
            if (cleanupFailure != null) {
                if (operationFailure != null) {
                    operationFailure.addSuppressed(cleanupFailure);
                } else {
                    throw RuntimeFailure.internal(
                            "community.sql_completion_context_cleanup_failed",
                            "the Community SQL-completion context could not be cleared",
                            cleanupFailure);
                }
            }
        }
    }

    private Object connectInfo(
            String canonicalDatabaseType,
            Connection connection,
            CompleteCommunitySqlRequest request)
            throws ReflectiveOperationException {
        Object connectInfo = connectInfoType.getDeclaredConstructor().newInstance();
        invokeSetter(connectInfo, "setDbType", String.class, canonicalDatabaseType);
        invokeSetter(connectInfo, "setDatabaseName", String.class, request.getDatabaseName());
        invokeSetter(connectInfo, "setSchemaName", String.class, request.getSchemaName());
        invokeSetter(connectInfo, "setAlias", String.class, request.getDatasourceName());
        invokeSetter(connectInfo, "setDataSourceId", Long.class, request.getDatasourceScope());
        invokeSetter(connectInfo, "setConsoleId", Long.class, request.getDatasourceScope());
        invokeSetter(connectInfo, "setConnection", Connection.class, connection);
        return connectInfo;
    }

    private Object completionRequest(CompleteCommunitySqlRequest request)
            throws ReflectiveOperationException {
        Object projected = requestType.getDeclaredConstructor().newInstance();
        invokeSetter(projected, "setConsoleId", Long.class, request.getDatasourceScope());
        invokeSetter(projected, "setDataSourceId", Long.class, request.getDatasourceScope());
        invokeSetter(projected, "setDatabaseName", String.class, request.getDatabaseName());
        invokeSetter(projected, "setSchemaName", String.class, request.getSchemaName());
        invokeSetter(projected, "setSql", String.class, request.getSql());
        invokeSetter(projected, "setCursor", Integer.class, request.getCursorUtf16());
        invokeSetter(projected, "setMinPrefixLength", Integer.class, request.getMinPrefixLength());
        invokeSetter(projected, "setNeedFullName", Boolean.class, request.getNeedFullName());
        invokeSetter(
                projected,
                "setKeywordCase",
                String.class,
                request.getKeywordCase().trim().toUpperCase(Locale.ROOT));
        if (request.hasActiveSnippetSlot()) {
            var slot = request.getActiveSnippetSlot();
            Constructor<?> constructor = activeSnippetSlotType.getConstructor(
                    String.class, Integer.class, Integer.class);
            Object projectedSlot = constructor.newInstance(
                    slot.getType().trim().toUpperCase(Locale.ROOT),
                    slot.getReplaceStartUtf16(),
                    slot.getReplaceEndUtf16());
            invokeSetter(projected, "setActiveSnippetSlot", activeSnippetSlotType, projectedSlot);
        }
        return projected;
    }

    private Throwable clearCompletionState(long datasourceScope) {
        Throwable failure = null;
        try {
            clearContextThreadLocal();
        } catch (ReflectiveOperationException | RuntimeException | LinkageError contextFailure) {
            failure = contextFailure;
        }
        try {
            clearCompletionCaches(datasourceScope);
        } catch (ReflectiveOperationException | RuntimeException | LinkageError cacheFailure) {
            if (failure == null) {
                failure = cacheFailure;
            } else {
                failure.addSuppressed(cacheFailure);
            }
        }
        return failure;
    }

    private void clearContextThreadLocal() throws ReflectiveOperationException {
        Field field = contextType.getDeclaredField("CONNECT_INFO_THREAD_LOCAL");
        field.setAccessible(true);
        Object value = field.get(null);
        if (!(value instanceof ThreadLocal<?> context)) {
            throw new IllegalStateException("Community connection context is not a ThreadLocal");
        }
        context.remove();
    }

    private void clearCompletionCaches(long datasourceScope) throws ReflectiveOperationException {
        Class<?> memoryCacheType = Class.forName(MEMORY_CACHE_CLASS, true, loader);
        Field cacheField = memoryCacheType.getDeclaredField("CACHE");
        cacheField.setAccessible(true);
        Object cache = cacheField.get(null);
        Class<?> cacheType = Class.forName("com.google.common.cache.Cache", true, loader);
        Object values = cacheType.getMethod("asMap").invoke(cache);
        if (!(values instanceof Map<?, ?> entries)) {
            throw new IllegalStateException("Community completion cache is not a map");
        }
        Method remove = memoryCacheType.getMethod("remove", String.class);
        for (Object keyValue : List.copyOf(entries.keySet())) {
            if (keyValue instanceof String key
                    && belongsToDatasourceScope(key, datasourceScope)) {
                remove.invoke(null, key);
            }
        }
    }

    static boolean belongsToDatasourceScope(String key, long datasourceScope) {
        String dataPrefix = "databases_datasourceId_" + datasourceScope + "_";
        return key.startsWith(dataPrefix) || key.startsWith("console_parser_" + dataPrefix);
    }

    static RuntimeFailure connectionOwnershipFailure(Connection connection) {
        try {
            if (connection.isClosed()) {
                return RuntimeFailure.internal(
                        "community.sql_completion_connection_closed",
                        "Community SQL completion closed the host-owned JDBC connection",
                        null);
            }
            return null;
        } catch (SQLException | RuntimeException | LinkageError failure) {
            return RuntimeFailure.internal(
                    "community.sql_completion_connection_state_failed",
                    "the host-owned JDBC connection state could not be verified",
                    failure);
        }
    }

    private static CommunitySqlCompletion project(Object response, String sql)
            throws ReflectiveOperationException, RuntimeFailure {
        ProjectionBudget budget = new ProjectionBudget();
        int start = nonNegativeInteger(response, "getReplaceStart", "replace_start_utf16");
        int end = nonNegativeInteger(response, "getReplaceEnd", "replace_end_utf16");
        requireUtf16Range(start, end, sql.length(), "completion replacement");
        CommunitySqlCompletion.Builder projected = CommunitySqlCompletion.newBuilder()
                .setStatus(requiredString(response, "getStatus", "status", budget))
                .setReplaceStartUtf16(start)
                .setReplaceEndUtf16(end);
        String reason = nullableString(response, "getReasonCode", "reason_code", budget);
        if (reason != null) {
            projected.setReasonCode(reason);
        }

        List<?> candidates = list(invoke(response, "getCandidates"), "completion candidates");
        requireCount(candidates.size(), MAX_CANDIDATES, "completion candidates");
        int snippetSlots = 0;
        for (Object candidate : candidates) {
            CommunitySqlCompletionCandidate result = candidate(candidate, sql, budget);
            snippetSlots = Math.addExact(snippetSlots, result.getSnippetSlotsCount());
            requireCount(snippetSlots, MAX_SNIPPET_SLOTS, "completion snippet slots");
            projected.addCandidates(result);
        }

        List<?> hints = list(invoke(response, "getEditorHints"), "completion editor hints");
        requireCount(hints.size(), MAX_EDITOR_HINTS, "completion editor hints");
        int hintItems = 0;
        for (Object hint : hints) {
            CommunitySqlCompletionEditorHint result = editorHint(hint, budget);
            hintItems = Math.addExact(hintItems, result.getItemsCount());
            requireCount(hintItems, MAX_EDITOR_HINT_ITEMS, "completion editor hint items");
            projected.addEditorHints(result);
        }

        CommunitySqlCompletion completion = projected.build();
        if (completion.getSerializedSize() > MAX_RESPONSE_BYTES) {
            throw RuntimeFailure.limit("Community SQL-completion response", MAX_RESPONSE_BYTES);
        }
        return completion;
    }

    private static CommunitySqlCompletionCandidate candidate(
            Object value, String sql, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunitySqlCompletionCandidate.Builder projected =
                CommunitySqlCompletionCandidate.newBuilder()
                        .setLabel(requiredString(value, "getLabel", "candidate_label", budget))
                        .setType(requiredString(value, "getType", "candidate_type", budget))
                        .setInsertType(requiredString(
                                value, "getInsertType", "candidate_insert_type", budget));
        setOptionalString(projected::setId, value, "getId", "candidate_id", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setInsertText, value, "getInsertText", "candidate_insert_text", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setDetail, value, "getDetail", "candidate_detail", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setDescription, value, "getDescription", "candidate_description", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setDataType, value, "getDataType", "candidate_data_type", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setObjectType, value, "getObjectType", "candidate_object_type", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setComment, value, "getComment", "candidate_comment", budget, MAX_COMMENT_BYTES);
        setOptionalString(projected::setDatasourceName, value, "getDatasourceName", "candidate_datasource_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setDatabaseName, value, "getDatabaseName", "candidate_database_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setSchemaName, value, "getSchemaName", "candidate_schema_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setTableName, value, "getTableName", "candidate_table_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setTableAlias, value, "getTableAlias", "candidate_table_alias", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setColumnName, value, "getColumnName", "candidate_column_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setObjectName, value, "getObjectName", "candidate_object_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setParameterMode, value, "getParameterMode", "candidate_parameter_mode", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setSortText, value, "getSortText", "candidate_sort_text", budget, ProtocolLimits.MAX_SCALAR_BYTES);

        Integer replaceStart = nullableInteger(value, "getReplaceStart");
        Integer replaceEnd = nullableInteger(value, "getReplaceEnd");
        if ((replaceStart == null) != (replaceEnd == null)) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_candidate_range_invalid",
                    "a completion candidate must provide both replacement endpoints");
        }
        if (replaceStart != null) {
            requireUtf16Range(replaceStart, replaceEnd, sql.length(), "candidate replacement");
            projected.setReplaceStartUtf16(replaceStart).setReplaceEndUtf16(replaceEnd);
        }
        Integer sortRank = nullableInteger(value, "getSortRank");
        if (sortRank != null) {
            projected.setSortRank(sortRank);
        }
        List<?> slots = list(invoke(value, "getSnippetSlots"), "candidate snippet slots");
        requireCount(slots.size(), MAX_SNIPPET_SLOTS, "candidate snippet slots");
        for (Object slot : slots) {
            projected.addSnippetSlots(projectString(slot, "candidate_snippet_slot", budget, ProtocolLimits.MAX_SCALAR_BYTES, true));
        }
        return projected.build();
    }

    private static CommunitySqlCompletionEditorHint editorHint(
            Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunitySqlCompletionEditorHint.Builder projected =
                CommunitySqlCompletionEditorHint.newBuilder()
                        .setType(requiredString(value, "getType", "editor_hint_type", budget));
        setOptionalRange(projected::setStatementRange, invoke(value, "getStatementRange"));
        setOptionalRange(projected::setRowRange, invoke(value, "getRowRange"));
        setOptionalRange(projected::setValueRange, invoke(value, "getValueRange"));
        List<?> items = list(invoke(value, "getItems"), "editor hint items");
        requireCount(items.size(), MAX_EDITOR_HINT_ITEMS, "editor hint items");
        for (Object item : items) {
            projected.addItems(editorHintItem(item, budget));
        }
        return projected.build();
    }

    private static CommunitySqlCompletionEditorHintItem editorHintItem(
            Object value, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        CommunitySqlCompletionEditorHintItem.Builder projected =
                CommunitySqlCompletionEditorHintItem.newBuilder()
                        .setRowIndex(nonNegativeInteger(value, "getRowIndex", "editor_hint_row_index"))
                        .setColumnIndex(nonNegativeInteger(value, "getColumnIndex", "editor_hint_column_index"))
                        .setActive(booleanValue(invoke(value, "isActive")));
        setOptionalString(projected::setFieldName, value, "getFieldName", "editor_hint_field_name", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setFieldType, value, "getFieldType", "editor_hint_field_type", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalString(projected::setLabel, value, "getLabel", "editor_hint_label", budget, ProtocolLimits.MAX_SCALAR_BYTES);
        setOptionalRange(projected::setRange, invoke(value, "getRange"));
        return projected.build();
    }

    private static CommunitySqlCompletionRange range(Object value)
            throws ReflectiveOperationException, RuntimeFailure {
        int startLine = positiveInteger(value, "getStartLineNumber", "range_start_line_number");
        int startColumn = positiveInteger(value, "getStartColumn", "range_start_column");
        int endLine = positiveInteger(value, "getEndLineNumber", "range_end_line_number");
        int endColumn = positiveInteger(value, "getEndColumn", "range_end_column");
        if (startLine > endLine || startLine == endLine && startColumn > endColumn) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_editor_range_invalid",
                    "an editor range start must not exceed its end");
        }
        return CommunitySqlCompletionRange.newBuilder()
                .setStartLineNumber(startLine)
                .setStartColumn(startColumn)
                .setEndLineNumber(endLine)
                .setEndColumn(endColumn)
                .build();
    }

    private static void setOptionalRange(RangeSetter setter, Object value)
            throws ReflectiveOperationException, RuntimeFailure {
        if (value != null) {
            setter.set(range(value));
        }
    }

    private static void setOptionalString(
            StringSetter setter,
            Object target,
            String getter,
            String field,
            ProjectionBudget budget,
            int maximumBytes)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        if (value != null) {
            setter.set(projectString(value, field, budget, maximumBytes, false));
        }
    }

    private static String requiredString(
            Object target, String getter, String field, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        return projectString(invoke(target, getter), field, budget, ProtocolLimits.MAX_SCALAR_BYTES, true);
    }

    private static String nullableString(
            Object target, String getter, String field, ProjectionBudget budget)
            throws ReflectiveOperationException, RuntimeFailure {
        Object value = invoke(target, getter);
        return value == null
                ? null
                : projectString(value, field, budget, ProtocolLimits.MAX_SCALAR_BYTES, false);
    }

    private static String projectString(
            Object value,
            String field,
            ProjectionBudget budget,
            int maximumBytes,
            boolean required)
            throws RuntimeFailure {
        String projected = value == null ? "" : String.valueOf(value);
        if (required && projected.isBlank()) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_" + field + "_invalid", field + " is required");
        }
        ProtocolLimits.requireUtf8(projected, maximumBytes, field);
        budget.consume(projected, field);
        return projected;
    }

    private static int nonNegativeInteger(Object target, String getter, String field)
            throws ReflectiveOperationException, RuntimeFailure {
        Integer value = nullableInteger(target, getter);
        if (value == null || value < 0) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_" + field + "_invalid",
                    field + " must be non-negative");
        }
        return value;
    }

    private static int positiveInteger(Object target, String getter, String field)
            throws ReflectiveOperationException, RuntimeFailure {
        int value = nonNegativeInteger(target, getter, field);
        if (value == 0) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_" + field + "_invalid",
                    field + " must be one-based");
        }
        return value;
    }

    private static Integer nullableInteger(Object target, String getter)
            throws ReflectiveOperationException {
        Object value = invoke(target, getter);
        return value == null ? null : ((Number) value).intValue();
    }

    private static boolean booleanValue(Object value) {
        return value instanceof Boolean flag && flag;
    }

    private static void requireUtf16Offset(int value, int maximum, String field)
            throws RuntimeFailure {
        if (value < 0 || value > maximum) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_" + field + "_invalid",
                    field + " must be inside the SQL UTF-16 boundary");
        }
    }

    private static void requireUtf16Range(int start, int end, int maximum, String field)
            throws RuntimeFailure {
        requireUtf16Offset(start, maximum, field + " start");
        requireUtf16Offset(end, maximum, field + " end");
        if (start > end) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_range_invalid", field + " start exceeds its end");
        }
    }

    static void requireCandidateCount(int count) throws RuntimeFailure {
        requireCount(count, MAX_CANDIDATES, "completion candidates");
    }

    static void requireDatasourceScope(long datasourceScope) throws RuntimeFailure {
        if (datasourceScope <= 0) {
            throw RuntimeFailure.validation(
                    "community.sql_completion_datasource_scope_invalid",
                    "datasource_scope must fit a positive Java long");
        }
    }

    static void requireEditorHintCount(int count) throws RuntimeFailure {
        requireCount(count, MAX_EDITOR_HINTS, "completion editor hints");
    }

    static void requireEditorHintItemCount(int count) throws RuntimeFailure {
        requireCount(count, MAX_EDITOR_HINT_ITEMS, "completion editor hint items");
    }

    static void requireSnippetSlotCount(int count) throws RuntimeFailure {
        requireCount(count, MAX_SNIPPET_SLOTS, "completion snippet slots");
    }

    private static void requireCount(int count, int maximum, String field) throws RuntimeFailure {
        if (count < 0 || count > maximum) {
            throw RuntimeFailure.limit(field, maximum);
        }
    }

    private static List<?> list(Object value, String field) throws RuntimeFailure {
        if (value == null) {
            return List.of();
        }
        if (value instanceof List<?> values) {
            return values;
        }
        throw RuntimeFailure.internal(
                "community.sql_completion_projection_failed",
                field + " did not return a list",
                null);
    }

    private static Object construct(
            ClassLoader loader, String className, Class<?>[] parameterTypes, Object... arguments)
            throws ReflectiveOperationException {
        Class<?> type = Class.forName(className, true, loader);
        return type.getConstructor(parameterTypes).newInstance(arguments);
    }

    private static InvocationHandler metadataTableService(ClassLoader loader) {
        return (proxy, method, arguments) -> switch (method.getName()) {
            case "queryColumns" -> queryColumns(loader, arguments);
            case "toString" -> "Chat2DBRustMetadataTableService";
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> arguments != null && arguments.length == 1 && proxy == arguments[0];
            default -> throw new UnsupportedOperationException(method.toGenericString());
        };
    }

    private static Object queryColumns(ClassLoader loader, Object[] arguments) throws Throwable {
        if (arguments == null || arguments.length != 1 || arguments[0] == null) {
            return List.of();
        }
        Object request = arguments[0];
        String databaseName = (String) invoke(request, "getDatabaseName");
        String schemaName = (String) invoke(request, "getSchemaName");
        String tableName = (String) invoke(request, "getTableName");
        Class<?> contextType = Class.forName(CONTEXT_CLASS, true, loader);
        Connection connection = (Connection) contextType.getMethod("getConnection").invoke(null);
        Object metadata = contextType.getMethod("getDbMetaData").invoke(null);
        Class<?> columnRequestType = Class.forName(COLUMN_METADATA_REQUEST_CLASS, true, loader);
        Object columnRequest = columnRequestType
                .getConstructor(String.class, String.class, String.class, String.class)
                .newInstance(databaseName, schemaName, tableName, null);
        Class<?> metadataType = Class.forName(DB_METADATA_CLASS, true, loader);
        try {
            Object columns = metadataType
                    .getMethod("columns", Connection.class, columnRequestType)
                    .invoke(metadata, connection, columnRequest);
            return columns == null ? List.of() : columns;
        } catch (InvocationTargetException failure) {
            throw rootInvocationCause(failure);
        }
    }

    private static Object invoke(Object target, String method)
            throws ReflectiveOperationException {
        if (target == null) {
            throw new IllegalStateException("Community SQL-completion invocation target is null");
        }
        Method reflected = target.getClass().getMethod(method);
        return reflected.invoke(target);
    }

    private static void invokeSetter(
            Object target, String method, Class<?> parameterType, Object value)
            throws ReflectiveOperationException {
        target.getClass().getMethod(method, parameterType).invoke(target, value);
    }

    private static Throwable rootInvocationCause(Throwable failure) {
        return failure instanceof InvocationTargetException invocation
                        && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
    }

    @FunctionalInterface
    private interface StringSetter {
        void set(String value);
    }

    @FunctionalInterface
    private interface RangeSetter {
        void set(CommunitySqlCompletionRange value);
    }

    private static final class ProjectionBudget {
        private int remaining = MAX_RESPONSE_BYTES;

        void consume(String value, String field) throws RuntimeFailure {
            int bytes = ProtocolLimits.utf8Length(value);
            if (bytes > remaining) {
                throw RuntimeFailure.limit(field, MAX_RESPONSE_BYTES);
            }
            remaining -= bytes;
        }
    }
}

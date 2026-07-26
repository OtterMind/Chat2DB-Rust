import {
  AlignLeft,
  AlertCircle,
  ChevronLeft,
  ChevronRight,
  CircleStop,
  Code2,
  Database,
  KeyRound,
  LoaderCircle,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Server,
  ShieldCheck,
  Trash2,
  X,
} from 'lucide-react';
import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  ApiRequestError,
  BackendClient,
  CommunitySqlAnalysis,
  CommunitySqlValidation,
  CreateDatasourceRequest,
  Datasource,
  DatasourceConnection,
  DatasourceConnectionProperty,
  DatasourceSecretChange,
  HealthResponse,
  JdbcValue,
  OperationEventEnvelope,
  OperationSnapshot,
  OperationSubscription,
  ResultPage,
  UpdateDatasourceRequest,
  createBackendClient,
  observeOperation,
} from './backend';
import { CommunityExplorer } from './CommunityExplorer';
import { isCurrentSqlFormatRequest } from './sql-format-model';

const PAGE_ROWS = 50n;
const PAGE_BYTES = '1048576';
const VALIDATION_PREVIEW_ITEMS = 8;

interface ConnectionPropertyForm extends DatasourceConnectionProperty {
  id: number;
}

interface DatasourceFormValue {
  name: string;
  driverId: string;
  connectionMode: 'none' | 'keep' | 'clear' | 'replace';
  jdbcUrl: string;
  readOnly: boolean;
  properties: ConnectionPropertyForm[];
}

interface QueryOperation {
  id: string;
  status: OperationSnapshot['status'] | 'starting';
  rowCount: string;
  byteCount: string;
  lastSequence?: string;
  resultId?: string;
  error?: string;
}

type SqlInspection =
  | { kind: 'analysis'; result: CommunitySqlAnalysis }
  | { kind: 'validation'; result: CommunitySqlValidation };

type DialogState = { kind: 'create' } | { kind: 'edit'; datasource: Datasource };

function errorMessage(error: unknown): string {
  if (error instanceof ApiRequestError) return `${error.apiError.code}: ${error.message}`;
  return error instanceof Error ? error.message : 'Unexpected runtime error';
}

function defaultProperties(): ConnectionPropertyForm[] {
  return [
    { id: 1, key: 'user', value: '', sensitive: false },
    { id: 2, key: 'password', value: '', sensitive: true },
  ];
}

function initialForm(dialog: DialogState): DatasourceFormValue {
  if (dialog.kind === 'edit') {
    return {
      name: dialog.datasource.name,
      driverId: dialog.datasource.driverId,
      connectionMode: 'keep',
      jdbcUrl: '',
      readOnly: false,
      properties: defaultProperties(),
    };
  }
  return {
    name: '',
    driverId: '',
    connectionMode: 'replace',
    jdbcUrl: '',
    readOnly: false,
    properties: defaultProperties(),
  };
}

function connectionFromForm(form: DatasourceFormValue): DatasourceConnection {
  return {
    jdbcUrl: form.jdbcUrl.trim(),
    readOnly: form.readOnly,
    properties: form.properties
      .filter((property) => property.key.trim().length > 0)
      .map(({ key, value, sensitive }) => ({ key: key.trim(), value, sensitive })),
  };
}

function formatCount(value: string): string {
  try {
    return new Intl.NumberFormat().format(BigInt(value));
  } catch {
    return value;
  }
}

function formatBytes(value: string): string {
  let bytes: bigint;
  try {
    bytes = BigInt(value);
  } catch {
    return value;
  }
  if (bytes < 1024n) return `${bytes} B`;
  if (bytes < 1024n * 1024n) return `${(Number(bytes) / 1024).toFixed(1)} KB`;
  return `${(Number(bytes) / (1024 * 1024)).toFixed(1)} MB`;
}

function formatJdbcValue(value: JdbcValue): string {
  switch (value.type) {
    case 'null':
      return 'NULL';
    case 'boolean':
      return value.value ? 'true' : 'false';
    case 'opaque':
      return value.displayValue;
    case 'binary':
      return value.value.length > 36 ? `${value.value.slice(0, 36)}...` : value.value;
    default:
      return value.value;
  }
}

function DatasourceDialog({
  dialog,
  busy,
  submissionError,
  onClose,
  onSubmit,
}: {
  dialog: DialogState;
  busy: boolean;
  submissionError: string | null;
  onClose: () => void;
  onSubmit: (value: DatasourceFormValue) => Promise<void>;
}) {
  const [form, setForm] = useState(() => initialForm(dialog));
  const [validation, setValidation] = useState<string | null>(null);
  const nextPropertyId = useRef(3);
  const isEdit = dialog.kind === 'edit';
  const showConnection = form.connectionMode === 'replace';

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.name.trim() || !form.driverId.trim()) {
      setValidation('Name and driver ID are required.');
      return;
    }
    if (showConnection && !form.jdbcUrl.trim()) {
      setValidation('JDBC URL is required when replacing the connection.');
      return;
    }
    setValidation(null);
    await onSubmit(form);
  };

  const updateProperty = (id: number, patch: Partial<ConnectionPropertyForm>) => {
    setForm((current) => ({
      ...current,
      properties: current.properties.map((property) => (
        property.id === id ? { ...property, ...patch } : property
      )),
    }));
  };

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="datasource-dialog-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div>
            <span className="section-kicker">Datasource</span>
            <h2 id="datasource-dialog-title">{isEdit ? 'Edit connection' : 'New connection'}</h2>
          </div>
          <button className="icon-button quiet" type="button" onClick={onClose} aria-label="Close" title="Close">
            <X size={18} aria-hidden="true" />
          </button>
        </header>

        <form onSubmit={(event) => void submit(event)}>
          <div className="form-grid two-columns">
            <label>
              <span>Name</span>
              <input
                autoFocus
                value={form.name}
                onChange={(event) => setForm({ ...form, name: event.target.value })}
                placeholder="Local analytics"
              />
            </label>
            <label>
              <span>Driver ID</span>
              <input
                value={form.driverId}
                onChange={(event) => setForm({ ...form, driverId: event.target.value })}
                placeholder="postgresql"
              />
            </label>
          </div>

          {isEdit ? (
            <fieldset className="secret-choice">
              <legend>Stored connection</legend>
              <div className="segmented-control">
                {(['keep', 'replace', 'clear'] as const).map((action) => (
                  <label key={action}>
                    <input
                      type="radio"
                      name="secret-action"
                      value={action}
                      checked={form.connectionMode === action}
                      onChange={() => setForm({ ...form, connectionMode: action })}
                    />
                    <span>{action}</span>
                  </label>
                ))}
              </div>
            </fieldset>
          ) : (
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={form.connectionMode === 'replace'}
                onChange={(event) => setForm({
                  ...form,
                  connectionMode: event.target.checked ? 'replace' : 'none',
                })}
              />
              <span>Store connection details</span>
            </label>
          )}

          {showConnection ? (
            <div className="connection-fields">
              <label>
                <span>JDBC URL</span>
                <input
                  value={form.jdbcUrl}
                  onChange={(event) => setForm({ ...form, jdbcUrl: event.target.value })}
                  placeholder="jdbc:postgresql://127.0.0.1:5432/app"
                  spellCheck={false}
                />
              </label>
              <label className="toggle-row compact">
                <input
                  type="checkbox"
                  checked={form.readOnly}
                  onChange={(event) => setForm({ ...form, readOnly: event.target.checked })}
                />
                <span>Open sessions read-only</span>
              </label>

              <div className="property-heading">
                <span>Connection properties</span>
                <button
                  className="text-button"
                  type="button"
                  onClick={() => setForm((current) => ({
                    ...current,
                    properties: [
                      ...current.properties,
                      { id: nextPropertyId.current++, key: '', value: '', sensitive: false },
                    ],
                  }))}
                >
                  <Plus size={14} aria-hidden="true" /> Add property
                </button>
              </div>
              <div className="property-list">
                {form.properties.map((property) => (
                  <div className="property-row" key={property.id}>
                    <input
                      aria-label="Property name"
                      value={property.key}
                      onChange={(event) => updateProperty(property.id, { key: event.target.value })}
                      placeholder="property"
                      spellCheck={false}
                    />
                    <input
                      aria-label="Property value"
                      type={property.sensitive ? 'password' : 'text'}
                      autoComplete={property.sensitive ? 'new-password' : 'off'}
                      value={property.value}
                      onChange={(event) => updateProperty(property.id, { value: event.target.value })}
                      placeholder="value"
                    />
                    <label className="sensitive-check" title="Sensitive value">
                      <input
                        type="checkbox"
                        checked={property.sensitive}
                        onChange={(event) => updateProperty(property.id, { sensitive: event.target.checked })}
                      />
                      <KeyRound size={15} aria-hidden="true" />
                    </label>
                    <button
                      className="icon-button quiet compact-button"
                      type="button"
                      onClick={() => setForm((current) => ({
                        ...current,
                        properties: current.properties.filter((item) => item.id !== property.id),
                      }))}
                      aria-label="Remove property"
                      title="Remove property"
                    >
                      <X size={15} aria-hidden="true" />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          {validation || submissionError ? (
            <p className="form-error" role="alert">{validation ?? submissionError}</p>
          ) : null}
          <footer className="dialog-actions">
            <button className="secondary-button" type="button" onClick={onClose} disabled={busy}>Cancel</button>
            <button className="primary-button" type="submit" disabled={busy}>
              {busy ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : null}
              {isEdit ? 'Save changes' : 'Create datasource'}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}

function ResultTable({ page, loading }: { page: ResultPage; loading: boolean }) {
  return (
    <div className="result-scroller" aria-busy={loading}>
      <table>
        <thead>
          <tr>
            <th className="row-number">#</th>
            {page.columns.map((column) => (
              <th key={`${column.ordinal}-${column.label}`}>
                <span>{column.label}</span>
                <small>{column.jdbcTypeName}</small>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {page.rows.map((row, rowIndex) => (
            <tr key={`${page.offset}-${rowIndex}`}>
              <td className="row-number">{formatCount((BigInt(page.offset) + BigInt(rowIndex) + 1n).toString())}</td>
              {row.values.map((value, valueIndex) => (
                <td
                  className={`value-${value.type}`}
                  key={`${valueIndex}-${value.type}`}
                  title={value.type === 'binary' ? value.value : undefined}
                >
                  {formatJdbcValue(value)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {page.rows.length === 0 ? <div className="empty-result">Query returned no rows.</div> : null}
    </div>
  );
}

export default function App() {
  const client = useMemo<BackendClient>(() => createBackendClient(), []);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [datasources, setDatasources] = useState<Datasource[]>([]);
  const [selectedId, setSelectedId] = useState<string>('');
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [dialogBusy, setDialogBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [loadingDatasources, setLoadingDatasources] = useState(true);
  const [sql, setSql] = useState('SELECT 1;');
  const [communitySelection, setCommunitySelection] = useState({ datasourceKey: '', databaseType: '' });
  const [communityParserAvailable, setCommunityParserAvailable] = useState(false);
  const [sqlInspection, setSqlInspection] = useState<SqlInspection | null>(null);
  const [inspectionLoading, setInspectionLoading] = useState<'analysis' | 'validation' | null>(null);
  const [formatLoading, setFormatLoading] = useState(false);
  const [operation, setOperation] = useState<QueryOperation | null>(null);
  const [resultPage, setResultPage] = useState<ResultPage | null>(null);
  const [resultLoading, setResultLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const subscriptionRef = useRef<OperationSubscription | null>(null);
  const resultRequestRef = useRef(0);
  const inspectionRequestRef = useRef(0);
  const formatRequestRef = useRef(0);

  const selectedDatasource = datasources.find((datasource) => datasource.id === selectedId);
  const selectedDatasourceKey = selectedDatasource
    ? `${selectedDatasource.id}:${selectedDatasource.revision}:${selectedDatasource.driverId}`
    : '';
  const communityDatabaseType = communitySelection.datasourceKey === selectedDatasourceKey
    ? communitySelection.databaseType
    : '';
  const communityCompatibility = health?.components.find(
    (component) => component.id === 'community-compatibility',
  );
  const currentFormatScope = {
    datasourceKey: selectedDatasourceKey,
    databaseType: communityDatabaseType,
    sql,
  };
  const formatScopeRef = useRef(currentFormatScope);
  formatScopeRef.current = currentFormatScope;
  const queryRunning = operation?.status === 'running' || operation?.status === 'starting';

  const updateSql = useCallback((nextSql: string) => {
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setSql(nextSql);
  }, []);

  const selectCommunityDatabaseType = useCallback((databaseType: string) => {
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setCommunityParserAvailable(false);
    setCommunitySelection({ datasourceKey: selectedDatasourceKey, databaseType });
  }, [selectedDatasourceKey]);

  const setParserAvailability = useCallback((available: boolean) => {
    if (!available) {
      inspectionRequestRef.current += 1;
      setInspectionLoading(null);
      setSqlInspection(null);
    }
    setCommunityParserAvailable(available);
  }, []);

  const selectDatasource = useCallback((datasourceId: string) => {
    formatRequestRef.current += 1;
    setFormatLoading(false);
    setSelectedId(datasourceId);
  }, []);

  const refreshDatasources = useCallback(async (signal?: AbortSignal) => {
    formatRequestRef.current += 1;
    setFormatLoading(false);
    setLoadingDatasources(true);
    try {
      const response = await client.listDatasources(signal);
      setDatasources(response.items);
      setSelectedId((current) => (
        response.items.some((item) => item.id === current) ? current : (response.items[0]?.id ?? '')
      ));
    } catch (requestError) {
      if (!(requestError instanceof DOMException && requestError.name === 'AbortError')) {
        setError(errorMessage(requestError));
      }
    } finally {
      if (!signal?.aborted) setLoadingDatasources(false);
    }
  }, [client]);

  useEffect(() => {
    const controller = new AbortController();
    void client.health(controller.signal).then(setHealth).catch((requestError: unknown) => {
      if (!(requestError instanceof DOMException && requestError.name === 'AbortError')) {
        setError(errorMessage(requestError));
      }
    });
    void refreshDatasources(controller.signal);
    return () => controller.abort();
  }, [client, refreshDatasources]);

  useEffect(() => () => subscriptionRef.current?.close(), []);

  useEffect(() => {
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setCommunityParserAvailable(false);
  }, [selectedDatasourceKey]);

  const saveDatasource = async (form: DatasourceFormValue) => {
    if (!dialog) return;
    setDialogBusy(true);
    setDialogError(null);
    setError(null);
    try {
      let saved: Datasource;
      if (dialog.kind === 'create') {
        const request: CreateDatasourceRequest = {
          name: form.name.trim(),
          driverId: form.driverId.trim(),
          connection: form.connectionMode === 'replace' ? connectionFromForm(form) : null,
        };
        saved = await client.createDatasource(request);
      } else {
        let secretChange: DatasourceSecretChange;
        if (form.connectionMode === 'replace') {
          secretChange = { action: 'replace', connection: connectionFromForm(form) };
        } else if (form.connectionMode === 'clear') {
          secretChange = { action: 'clear' };
        } else {
          secretChange = { action: 'keep' };
        }
        const request: UpdateDatasourceRequest = {
          expectedRevision: dialog.datasource.revision,
          name: form.name.trim(),
          driverId: form.driverId.trim(),
          secretChange,
        };
        saved = await client.updateDatasource(dialog.datasource.id, request);
      }
      setDialog(null);
      await refreshDatasources();
      setSelectedId(saved.id);
    } catch (requestError) {
      setDialogError(errorMessage(requestError));
    } finally {
      setDialogBusy(false);
    }
  };

  const deleteDatasource = async (datasource: Datasource) => {
    if (!window.confirm(`Delete datasource "${datasource.name}"?`)) return;
    setError(null);
    try {
      await client.deleteDatasource(datasource.id, datasource.revision);
      await refreshDatasources();
    } catch (requestError) {
      setError(errorMessage(requestError));
    }
  };

  const loadResultPage = useCallback(async (resultId: string, offset: string) => {
    const requestId = ++resultRequestRef.current;
    setResultLoading(true);
    try {
      const page = await client.resultPage(resultId, {
        offset,
        maxRows: PAGE_ROWS.toString(),
        maxBytes: PAGE_BYTES,
      });
      if (resultRequestRef.current === requestId) setResultPage(page);
    } catch (requestError) {
      if (resultRequestRef.current === requestId) setError(errorMessage(requestError));
    } finally {
      if (resultRequestRef.current === requestId) setResultLoading(false);
    }
  }, [client]);

  const applySnapshot = useCallback((snapshot: OperationSnapshot) => {
    setOperation({
      id: snapshot.operationId,
      status: snapshot.status,
      rowCount: snapshot.rowCount,
      byteCount: snapshot.byteCount,
      lastSequence: snapshot.lastSequence,
      resultId: snapshot.result?.id,
      error: snapshot.error?.message,
    });
    if (snapshot.result) void loadResultPage(snapshot.result.id, '0');
  }, [loadResultPage]);

  const recoverOperation = useCallback(async (operationId: string) => {
    try {
      applySnapshot(await client.operationSnapshot(operationId));
    } catch (requestError) {
      setError(errorMessage(requestError));
    }
  }, [applySnapshot, client]);

  const applyOperationEvent = useCallback((envelope: OperationEventEnvelope) => {
    const event = envelope.event;
    setOperation((current) => {
      const base: QueryOperation = current ?? {
        id: envelope.operationId,
        status: 'running',
        rowCount: '0',
        byteCount: '0',
      };
      switch (event.type) {
        case 'started':
          return { ...base, status: 'running', lastSequence: envelope.sequence };
        case 'progress':
          return {
            ...base,
            status: 'running',
            rowCount: event.rowCount,
            byteCount: event.byteCount,
            lastSequence: envelope.sequence,
          };
        case 'completed':
          return {
            ...base,
            status: 'completed',
            rowCount: event.result.rowCount,
            byteCount: event.result.byteCount,
            lastSequence: envelope.sequence,
            resultId: event.result.id,
          };
        case 'failed':
          return {
            ...base,
            status: 'failed',
            lastSequence: envelope.sequence,
            error: event.error.message,
          };
        case 'cancelled':
          return {
            ...base,
            status: 'cancelled',
            lastSequence: envelope.sequence,
            error: event.reason ?? undefined,
          };
      }
    });
    if (event.type === 'completed') void loadResultPage(event.result.id, '0');
    if (event.type === 'failed') setError(errorMessage(new ApiRequestError(event.error)));
  }, [loadResultPage]);

  const runQuery = async () => {
    if (!selectedDatasource || !sql.trim() || queryRunning) return;
    subscriptionRef.current?.close();
    subscriptionRef.current = null;
    resultRequestRef.current += 1;
    setResultLoading(false);
    setError(null);
    setResultPage(null);
    setOperation(null);
    let accepted;
    try {
      accepted = await client.startQuery({
        datasourceId: selectedDatasource.id,
        sql,
        parameters: [],
        limits: {
          maxRows: '100000',
          maxResultBytes: '67108864',
          batchRows: 500,
          batchBytes: 1048576,
          resultTtlSeconds: 3600,
        },
      });
      setOperation({
        id: accepted.operationId,
        status: 'starting',
        rowCount: '0',
        byteCount: '0',
      });
    } catch (requestError) {
      setError(errorMessage(requestError));
      setOperation(null);
      return;
    }
    subscriptionRef.current = observeOperation(client, accepted.operationId, {
        onEvent: applyOperationEvent,
        onSnapshot: applySnapshot,
        onError: (streamError) => setError(errorMessage(streamError)),
      });
  };

  const cancelQuery = async () => {
    if (!operation || !queryRunning) return;
    try {
      const response = await client.cancelOperation(operation.id);
      if (response.disposition !== 'accepted') await recoverOperation(operation.id);
    } catch (requestError) {
      setError(errorMessage(requestError));
    }
  };

  const analyzeSql = async () => {
    if (
      !sql.trim()
      || !communityDatabaseType
      || !communityParserAvailable
      || communityCompatibility?.state !== 'ready'
      || inspectionLoading !== null
    ) return;
    const requestId = ++inspectionRequestRef.current;
    setInspectionLoading('analysis');
    setSqlInspection(null);
    setError(null);
    try {
      const analysis = await client.parseCommunitySql({
        databaseType: communityDatabaseType,
        sql,
      });
      if (inspectionRequestRef.current === requestId) {
        setSqlInspection({ kind: 'analysis', result: analysis });
      }
    } catch (requestError) {
      if (inspectionRequestRef.current === requestId) setError(errorMessage(requestError));
    } finally {
      if (inspectionRequestRef.current === requestId) setInspectionLoading(null);
    }
  };

  const validateSql = async () => {
    if (
      !sql.trim()
      || !communityDatabaseType
      || !communityParserAvailable
      || communityCompatibility?.state !== 'ready'
      || inspectionLoading !== null
    ) return;
    const requestId = ++inspectionRequestRef.current;
    setInspectionLoading('validation');
    setSqlInspection(null);
    setError(null);
    try {
      const validation = await client.validateCommunitySql({
        databaseType: communityDatabaseType,
        sql,
      });
      if (inspectionRequestRef.current === requestId) {
        setSqlInspection({ kind: 'validation', result: validation });
      }
    } catch (requestError) {
      if (inspectionRequestRef.current === requestId) setError(errorMessage(requestError));
    } finally {
      if (inspectionRequestRef.current === requestId) setInspectionLoading(null);
    }
  };

  const formatSql = async () => {
    if (
      !sql.trim()
      || !communityDatabaseType
      || communityCompatibility?.state !== 'ready'
      || formatLoading
    ) return;
    const sourceSql = sql;
    const request = {
      sequence: ++formatRequestRef.current,
      scope: currentFormatScope,
    };
    setFormatLoading(true);
    setError(null);
    try {
      const formatted = await client.formatCommunitySql({
        databaseType: communityDatabaseType,
        sql: sourceSql,
      });
      if (!isCurrentSqlFormatRequest(
        request,
        formatRequestRef.current,
        formatScopeRef.current,
      )) return;
      if (formatted.sql !== sourceSql) {
        inspectionRequestRef.current += 1;
        setInspectionLoading(null);
        setSqlInspection(null);
        setSql(formatted.sql);
      }
    } catch (requestError) {
      if (isCurrentSqlFormatRequest(
        request,
        formatRequestRef.current,
        formatScopeRef.current,
      )) setError(errorMessage(requestError));
    } finally {
      if (formatRequestRef.current === request.sequence) setFormatLoading(false);
    }
  };

  const pageBack = () => {
    if (!resultPage || !operation?.resultId) return;
    const current = BigInt(resultPage.offset);
    const offset = current > PAGE_ROWS ? current - PAGE_ROWS : 0n;
    void loadResultPage(operation.resultId, offset.toString());
  };

  const pageForward = () => {
    if (!resultPage?.hasMore || !operation?.resultId) return;
    const offset = BigInt(resultPage.offset) + BigInt(resultPage.rows.length);
    void loadResultPage(operation.resultId, offset.toString());
  };

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark"><Database size={19} aria-hidden="true" /></span>
          <div><strong>Chat2DB</strong><span>Community</span></div>
        </div>
        <div className="runtime-state" title={health?.components.map((item) => `${item.label}: ${item.state}`).join('\n')}>
          <span className={`runtime-dot status-${health?.status ?? 'unavailable'}`} />
          <span>{health?.status ?? 'offline'}</span>
          <small>{client.transport === 'tauri' ? 'Desktop' : 'Web'}</small>
        </div>
      </header>

      <aside className="datasource-panel">
        <div className="panel-heading">
          <div><span className="section-kicker">Workspace</span><h1>Datasources</h1></div>
          <button className="icon-button" type="button" onClick={() => { setDialogError(null); setDialog({ kind: 'create' }); }} aria-label="New datasource" title="New datasource">
            <Plus size={18} aria-hidden="true" />
          </button>
        </div>
        <div className="datasource-list" aria-busy={loadingDatasources}>
          {loadingDatasources && datasources.length === 0 ? (
            <div className="list-state"><LoaderCircle className="spinning" size={18} /> Loading</div>
          ) : null}
          {!loadingDatasources && datasources.length === 0 ? (
            <div className="list-state"><Server size={20} /><span>No datasources</span></div>
          ) : null}
          {datasources.map((datasource) => (
            <button
              className={`datasource-item ${selectedId === datasource.id ? 'active' : ''}`}
              type="button"
              onClick={() => selectDatasource(datasource.id)}
              key={datasource.id}
            >
              <Database size={16} aria-hidden="true" />
              <span><strong>{datasource.name}</strong><small>{datasource.driverId}</small></span>
              <i className={datasource.hasSecret ? 'secret-ready' : ''} title={datasource.hasSecret ? 'Connection stored' : 'Connection missing'} />
            </button>
          ))}
        </div>
        <button className="refresh-button" type="button" onClick={() => void refreshDatasources()} disabled={loadingDatasources}>
          <RefreshCw className={loadingDatasources ? 'spinning' : undefined} size={15} aria-hidden="true" />
          Refresh
        </button>
      </aside>

      <CommunityExplorer
        client={client}
        datasource={selectedDatasource}
        compatibility={communityCompatibility}
        databaseType={communityDatabaseType}
        onDatabaseTypeChange={selectCommunityDatabaseType}
        onParserAvailabilityChange={setParserAvailability}
        onInsertSql={updateSql}
      />

      <main className="workspace">
        <section className="connection-bar">
          <div className="connection-identity">
            <span className={`connection-icon ${selectedDatasource?.hasSecret ? 'connected' : ''}`}>
              <Database size={18} aria-hidden="true" />
            </span>
            <div>
              <strong>{selectedDatasource?.name ?? 'Select a datasource'}</strong>
              <span>{selectedDatasource ? `${selectedDatasource.driverId} · revision ${selectedDatasource.revision}` : 'No active connection'}</span>
            </div>
          </div>
          {selectedDatasource ? (
            <div className="connection-actions">
              <button className="icon-button" type="button" onClick={() => { setDialogError(null); setDialog({ kind: 'edit', datasource: selectedDatasource }); }} aria-label="Edit datasource" title="Edit datasource">
                <Pencil size={16} aria-hidden="true" />
              </button>
              <button className="icon-button danger" type="button" onClick={() => void deleteDatasource(selectedDatasource)} aria-label="Delete datasource" title="Delete datasource">
                <Trash2 size={16} aria-hidden="true" />
              </button>
            </div>
          ) : null}
        </section>

        {error ? (
          <div className="error-band" role="alert">
            <AlertCircle size={17} aria-hidden="true" />
            <span>{error}</span>
            <button className="icon-button quiet compact-button" type="button" onClick={() => setError(null)} aria-label="Dismiss error" title="Dismiss error">
              <X size={15} aria-hidden="true" />
            </button>
          </div>
        ) : null}

        <section className="query-editor" aria-label="SQL editor">
          <div className="editor-toolbar">
            <div><span className="section-kicker">SQL console</span><strong>Query</strong></div>
            <div className="query-actions">
              <button
                className="secondary-button"
                type="button"
                onClick={() => void formatSql()}
                disabled={
                  formatLoading
                  || !sql.trim()
                  || !communityDatabaseType
                  || communityCompatibility?.state !== 'ready'
                }
              >
                {formatLoading ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <AlignLeft size={16} aria-hidden="true" />}
                {formatLoading ? 'Formatting' : 'Format'}
              </button>
              <button
                className="secondary-button"
                type="button"
                onClick={() => void analyzeSql()}
                disabled={
                  inspectionLoading !== null
                  || !sql.trim()
                  || !communityDatabaseType
                  || !communityParserAvailable
                  || communityCompatibility?.state !== 'ready'
                }
              >
                {inspectionLoading === 'analysis' ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <Code2 size={16} aria-hidden="true" />}
                {inspectionLoading === 'analysis' ? 'Analyzing' : 'Analyze'}
              </button>
              <button
                className="secondary-button"
                type="button"
                onClick={() => void validateSql()}
                disabled={
                  inspectionLoading !== null
                  || !sql.trim()
                  || !communityDatabaseType
                  || !communityParserAvailable
                  || communityCompatibility?.state !== 'ready'
                }
              >
                {inspectionLoading === 'validation' ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <ShieldCheck size={16} aria-hidden="true" />}
                {inspectionLoading === 'validation' ? 'Validating' : 'Validate'}
              </button>
              {queryRunning ? (
                <button className="secondary-button danger-text" type="button" onClick={() => void cancelQuery()}>
                  <CircleStop size={16} aria-hidden="true" /> Cancel
                </button>
              ) : null}
              <button className="primary-button" type="button" onClick={() => void runQuery()} disabled={!selectedDatasource || !selectedDatasource.hasSecret || !sql.trim() || queryRunning}>
                {queryRunning ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <Play size={16} fill="currentColor" aria-hidden="true" />}
                {queryRunning ? 'Running' : 'Run'}
              </button>
            </div>
          </div>
          <textarea
            value={sql}
            onChange={(event) => updateSql(event.target.value)}
            spellCheck={false}
            aria-label="SQL query"
          />
          {sqlInspection?.kind === 'analysis' ? (
            <section className="analysis-strip" aria-label="Community SQL analysis">
              <header>
                <strong>{sqlInspection.result.isSelect ? 'SELECT' : 'NON-SELECT'}</strong>
                <span>{sqlInspection.result.statements.length} statement{sqlInspection.result.statements.length === 1 ? '' : 's'}</span>
              </header>
              <div className="analysis-statements">
                {sqlInspection.result.statements.slice(0, 4).map((statement, index) => (
                  <div key={`${statement.statementType}:${index}`}>
                    <code>{statement.statementType || statement.kind}</code>
                    <span title={statement.sql}>{statement.sql}</span>
                  </div>
                ))}
                {sqlInspection.result.statements.length === 0 ? <span>No statements returned</span> : null}
                {sqlInspection.result.statements.length > 4 ? <span>+{sqlInspection.result.statements.length - 4} more</span> : null}
              </div>
            </section>
          ) : null}
          {sqlInspection?.kind === 'validation' ? (
            <section
              className={`analysis-strip validation-strip ${sqlInspection.result.valid ? 'validation-valid' : 'validation-invalid'}`}
              aria-label="Community SQL validation"
              aria-live="polite"
            >
              <header>
                <strong>{sqlInspection.result.valid ? 'VALID' : 'INVALID'}</strong>
                <span>
                  {sqlInspection.result.diagnostics.length} issue{sqlInspection.result.diagnostics.length === 1 ? '' : 's'}
                </span>
              </header>
              <div className="analysis-statements validation-diagnostics">
                {sqlInspection.result.valid
                  ? sqlInspection.result.statements.slice(0, 4).map((statement, index) => (
                    <div key={`${statement.statementType}:${index}`}>
                      <code>{statement.statementType || statement.kind}</code>
                      <span title={statement.sql}>{statement.sql}</span>
                    </div>
                  ))
                  : sqlInspection.result.diagnostics.slice(0, VALIDATION_PREVIEW_ITEMS).map((diagnostic, index) => (
                    <div key={`${diagnostic.startLine}:${diagnostic.startColumn}:${index}`}>
                      <code>
                        {diagnostic.startLine}:{diagnostic.startColumn}-{diagnostic.endLine}:{diagnostic.endColumn}
                      </code>
                      <span title={`${diagnostic.message}${diagnostic.tokenText ? ` (${diagnostic.tokenText})` : ''}`}>
                        {diagnostic.message}{diagnostic.tokenText ? ` · ${diagnostic.tokenText}` : ''}
                      </span>
                    </div>
                  ))}
                {sqlInspection.result.valid && sqlInspection.result.statements.length === 0 ? <span>No syntax issues</span> : null}
                {!sqlInspection.result.valid && sqlInspection.result.diagnostics.length === 0 ? <span>No diagnostics returned</span> : null}
                {sqlInspection.result.valid && sqlInspection.result.statements.length > 4 ? <span>+{sqlInspection.result.statements.length - 4} more</span> : null}
                {!sqlInspection.result.valid && sqlInspection.result.diagnostics.length > VALIDATION_PREVIEW_ITEMS ? (
                  <span>+{sqlInspection.result.diagnostics.length - VALIDATION_PREVIEW_ITEMS} more issues</span>
                ) : null}
              </div>
            </section>
          ) : null}
          <footer className="editor-status">
            <span>{selectedDatasource?.hasSecret ? `Ready${communityDatabaseType ? ` · ${communityDatabaseType}` : ''}` : 'Connection details required'}</span>
            {operation ? <code>{operation.id}</code> : null}
          </footer>
        </section>

        <section className="results-panel" aria-label="Query results">
          <header className="results-heading">
            <div>
              <span className="section-kicker">Output</span>
              <strong>Results</strong>
            </div>
            {operation ? (
              <dl className="operation-stats">
                <div><dt>Status</dt><dd className={`operation-${operation.status}`}>{operation.status}</dd></div>
                <div><dt>Rows</dt><dd>{formatCount(operation.rowCount)}</dd></div>
                <div><dt>Size</dt><dd>{formatBytes(operation.byteCount)}</dd></div>
              </dl>
            ) : null}
            {resultPage ? (
              <div className="pagination">
                <span>{resultPage.rows.length === 0 ? '0-0' : `${formatCount((BigInt(resultPage.offset) + 1n).toString())}-${formatCount((BigInt(resultPage.offset) + BigInt(resultPage.rows.length)).toString())}`} of {formatCount(resultPage.metadata.rowCount)}</span>
                <button className="icon-button compact-button" type="button" onClick={pageBack} disabled={resultLoading || resultPage.offset === '0'} aria-label="Previous page" title="Previous page">
                  <ChevronLeft size={16} aria-hidden="true" />
                </button>
                <button className="icon-button compact-button" type="button" onClick={pageForward} disabled={resultLoading || !resultPage.hasMore} aria-label="Next page" title="Next page">
                  <ChevronRight size={16} aria-hidden="true" />
                </button>
              </div>
            ) : null}
          </header>
          {resultPage ? (
            <ResultTable page={resultPage} loading={resultLoading} />
          ) : (
            <div className="empty-workspace">
              {queryRunning ? <LoaderCircle className="spinning" size={22} aria-hidden="true" /> : <Database size={24} aria-hidden="true" />}
              <span>{queryRunning ? `Persisting ${formatCount(operation?.rowCount ?? '0')} rows` : (operation?.error ?? 'No result set')}</span>
            </div>
          )}
        </section>
      </main>

      {dialog ? (
        <DatasourceDialog
          dialog={dialog}
          busy={dialogBusy}
          submissionError={dialogError}
          onClose={() => { if (!dialogBusy) { setDialog(null); setDialogError(null); } }}
          onSubmit={saveDatasource}
        />
      ) : null}
    </div>
  );
}

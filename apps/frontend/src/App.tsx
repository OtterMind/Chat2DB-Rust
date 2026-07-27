import {
  AlignLeft,
  AlertCircle,
  ChevronLeft,
  ChevronRight,
  CircleStop,
  Code2,
  Database,
  KeyRound,
  ListPlus,
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
import {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

import {
  ApiRequestError,
  BackendClient,
  CommunitySqlAnalysis,
  CommunitySqlCompletion,
  CommunitySqlCompletionCandidate,
  CommunitySqlValidation,
  CreateDatasourceRequest,
  Datasource,
  DatasourceConnection,
  DatasourceConnectionProperty,
  DatasourceSecretChange,
  HealthResponse,
  JdbcDriver,
  JdbcValue,
  OperationEventEnvelope,
  OperationSnapshot,
  OperationSubscription,
  ResultPage,
  StartCommunityTablePreviewRequest,
  UpdateDatasourceRequest,
  createBackendClient,
  observeOperation,
} from './backend';
import { CommunityCompletionContext, CommunityExplorer } from './CommunityExplorer';
import {
  applySqlCompletion,
  isCurrentSqlCompletionRequest,
  moveSqlCompletionSelection,
} from './sql-completion-model';
import { isCurrentSqlFormatRequest } from './sql-format-model';

const PAGE_ROWS = 50n;
const PAGE_BYTES = '8388608';
const VALIDATION_PREVIEW_ITEMS = 8;
const MAX_VISIBLE_COMPLETION_CANDIDATES = 200;
const INITIAL_SQL = 'SELECT 1;';
const INITIAL_COMPLETION_CONTEXT: CommunityCompletionContext = {
  databaseName: '',
  schemaName: '',
  refreshGeneration: 0,
};

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

type SqlCompletionState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'empty'; message: string }
  | { status: 'success'; result: CommunitySqlCompletion; selectedIndex: number };

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

function initialForm(dialog: DialogState, defaultDriverId = ''): DatasourceFormValue {
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
    driverId: defaultDriverId,
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

function driverOptionLabel(driver: JdbcDriver): string {
  return `${driver.name} ${driver.version} \u00b7 ${driver.driverClass}`;
}

export function DatasourceDialog({
  dialog,
  drivers,
  driversLoading,
  driversError,
  busy,
  submissionError,
  onRetryDrivers,
  onClose,
  onSubmit,
}: {
  dialog: DialogState;
  drivers: JdbcDriver[];
  driversLoading: boolean;
  driversError: string | null;
  busy: boolean;
  submissionError: string | null;
  onRetryDrivers: () => void;
  onClose: () => void;
  onSubmit: (value: DatasourceFormValue) => Promise<void>;
}) {
  const [form, setForm] = useState(() => initialForm(dialog, drivers[0]?.driverId));
  const [validation, setValidation] = useState<string | null>(null);
  const nextPropertyId = useRef(3);
  const isEdit = dialog.kind === 'edit';
  const showConnection = form.connectionMode === 'replace';
  const currentDriverMissing = isEdit && form.driverId.length > 0
    && !drivers.some((driver) => driver.driverId === form.driverId);
  const driverStatus = driversError
    ? `Could not load installed drivers: ${driversError}`
    : driversLoading
      ? (drivers.length > 0 ? 'Refreshing installed drivers...' : 'Loading installed drivers...')
      : drivers.length === 0
        ? 'No installed JDBC drivers found.'
        : null;

  useEffect(() => {
    if (!isEdit && !form.driverId && drivers[0]) {
      setForm((current) => ({ ...current, driverId: current.driverId || drivers[0].driverId }));
    }
  }, [drivers, form.driverId, isEdit]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!form.name.trim() || !form.driverId.trim()) {
      setValidation('Name and driver are required.');
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
                aria-label="Datasource name"
                value={form.name}
                onChange={(event) => setForm({ ...form, name: event.target.value })}
                placeholder="Local analytics"
              />
            </label>
            <div className="driver-field">
              <label>
                <span>Driver</span>
                <select
                  aria-label="Driver"
                  aria-describedby={driverStatus ? 'driver-inventory-status' : undefined}
                  value={form.driverId}
                  onChange={(event) => setForm({ ...form, driverId: event.target.value })}
                  disabled={busy || (!form.driverId && drivers.length === 0)}
                >
                  {!form.driverId ? (
                    <option value="" disabled>
                      {driversLoading ? 'Loading installed drivers...' : 'Select an installed driver'}
                    </option>
                  ) : null}
                  {currentDriverMissing ? (
                    <option value={form.driverId}>
                      {`Current driver (not installed) \u00b7 ${form.driverId}`}
                    </option>
                  ) : null}
                  {drivers.map((driver) => (
                    <option value={driver.driverId} key={driver.driverId}>
                      {driverOptionLabel(driver)}
                    </option>
                  ))}
                </select>
              </label>
              {driverStatus ? (
                <div
                  className={`driver-inventory-status ${driversError ? 'error' : ''}`}
                  id="driver-inventory-status"
                  role={driversError ? 'alert' : 'status'}
                >
                  {driversLoading ? <LoaderCircle className="spinning" size={14} aria-hidden="true" /> : null}
                  {driversError ? <AlertCircle size={14} aria-hidden="true" /> : null}
                  <span>
                    {driverStatus}
                    {driversError && isEdit && form.driverId
                      ? ' The existing driver remains available for this edit.'
                      : ''}
                  </span>
                  {!driversLoading && (driversError || drivers.length === 0) ? (
                    <button className="text-button" type="button" onClick={onRetryDrivers}>
                      {driversError ? 'Retry' : 'Refresh'}
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>
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
                  aria-label="JDBC URL"
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
            <button className="primary-button" type="submit" disabled={busy || !form.driverId}>
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

export default function App({ client: providedClient }: { client?: BackendClient } = {}) {
  const client = useMemo<BackendClient>(() => providedClient ?? createBackendClient(), [providedClient]);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [drivers, setDrivers] = useState<JdbcDriver[]>([]);
  const [driversLoading, setDriversLoading] = useState(true);
  const [driversError, setDriversError] = useState<string | null>(null);
  const [datasources, setDatasources] = useState<Datasource[]>([]);
  const [selectedId, setSelectedId] = useState<string>('');
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [dialogBusy, setDialogBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [loadingDatasources, setLoadingDatasources] = useState(true);
  const [sql, setSql] = useState(INITIAL_SQL);
  const [communitySelection, setCommunitySelection] = useState({ datasourceKey: '', databaseType: '' });
  const [communityCompletionContext, setCommunityCompletionContext] = useState(INITIAL_COMPLETION_CONTEXT);
  const [communityParserAvailable, setCommunityParserAvailable] = useState(false);
  const [sqlInspection, setSqlInspection] = useState<SqlInspection | null>(null);
  const [inspectionLoading, setInspectionLoading] = useState<'analysis' | 'validation' | null>(null);
  const [formatLoading, setFormatLoading] = useState(false);
  const [sqlCompletion, setSqlCompletion] = useState<SqlCompletionState>({ status: 'idle' });
  const [editorCursorUtf16, setEditorCursorUtf16] = useState(INITIAL_SQL.length);
  const [operation, setOperation] = useState<QueryOperation | null>(null);
  const [previewStarting, setPreviewStarting] = useState(false);
  const [resultPage, setResultPage] = useState<ResultPage | null>(null);
  const [resultLoading, setResultLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const subscriptionRef = useRef<OperationSubscription | null>(null);
  const activeOperationIdRef = useRef<string | null>(null);
  const previewRequestSequenceRef = useRef(0);
  const previewStartingRef = useRef(false);
  const driverRequestRef = useRef(0);
  const resultRequestRef = useRef(0);
  const inspectionRequestRef = useRef(0);
  const formatRequestRef = useRef(0);
  const completionRequestRef = useRef(0);
  const sqlEditorRef = useRef<HTMLTextAreaElement | null>(null);
  const editorCursorRef = useRef(INITIAL_SQL.length);
  const completionContextRef = useRef(INITIAL_COMPLETION_CONTEXT);
  const completionOptionRefs = useRef<Array<HTMLButtonElement | null>>([]);

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
  const currentCompletionScope = {
    datasourceKey: selectedDatasourceKey,
    databaseType: communityDatabaseType,
    databaseName: communityCompletionContext.databaseName,
    schemaName: communityCompletionContext.schemaName,
    sql,
    cursorUtf16: editorCursorUtf16,
    refreshGeneration: communityCompletionContext.refreshGeneration,
  };
  const completionScopeRef = useRef(currentCompletionScope);
  completionScopeRef.current = currentCompletionScope;
  const visibleCompletionCandidates = sqlCompletion.status === 'success'
    ? sqlCompletion.result.candidates.slice(0, MAX_VISIBLE_COMPLETION_CANDIDATES)
    : [];
  const queryRunning = operation?.status === 'running' || operation?.status === 'starting';

  const invalidateSqlCompletion = useCallback(() => {
    completionRequestRef.current += 1;
    setSqlCompletion((current) => current.status === 'idle' ? current : { status: 'idle' });
  }, []);

  const trackEditorCursor = useCallback((cursorUtf16: number, sqlLength: number) => {
    const safeCursor = Math.max(0, Math.min(cursorUtf16, sqlLength));
    if (editorCursorRef.current === safeCursor) return;
    editorCursorRef.current = safeCursor;
    setEditorCursorUtf16(safeCursor);
    invalidateSqlCompletion();
  }, [invalidateSqlCompletion]);

  const updateSql = useCallback((nextSql: string, nextCursorUtf16 = nextSql.length) => {
    completionRequestRef.current += 1;
    setSqlCompletion((current) => current.status === 'idle' ? current : { status: 'idle' });
    const safeCursor = Math.max(0, Math.min(nextCursorUtf16, nextSql.length));
    editorCursorRef.current = safeCursor;
    setEditorCursorUtf16(safeCursor);
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setSql(nextSql);
  }, []);

  const resetCompletionContext = useCallback(() => {
    const context = {
      databaseName: '',
      schemaName: '',
      refreshGeneration: completionContextRef.current.refreshGeneration,
    };
    completionContextRef.current = context;
    setCommunityCompletionContext(context);
  }, []);

  const selectCommunityDatabaseType = useCallback((databaseType: string) => {
    invalidateSqlCompletion();
    resetCompletionContext();
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setCommunityParserAvailable(false);
    setCommunitySelection({ datasourceKey: selectedDatasourceKey, databaseType });
  }, [invalidateSqlCompletion, resetCompletionContext, selectedDatasourceKey]);

  const setCompletionContext = useCallback((context: CommunityCompletionContext) => {
    const current = completionContextRef.current;
    if (
      current.databaseName === context.databaseName
      && current.schemaName === context.schemaName
      && current.refreshGeneration === context.refreshGeneration
    ) return;
    completionContextRef.current = context;
    invalidateSqlCompletion();
    setCommunityCompletionContext(context);
  }, [invalidateSqlCompletion]);

  const setParserAvailability = useCallback((available: boolean) => {
    if (!available) {
      inspectionRequestRef.current += 1;
      setInspectionLoading(null);
      setSqlInspection(null);
    }
    setCommunityParserAvailable(available);
  }, []);

  const selectDatasource = useCallback((datasourceId: string) => {
    if (datasourceId === selectedId) return;
    invalidateSqlCompletion();
    resetCompletionContext();
    formatRequestRef.current += 1;
    setFormatLoading(false);
    setSelectedId(datasourceId);
  }, [invalidateSqlCompletion, resetCompletionContext, selectedId]);

  const refreshDatasources = useCallback(async (signal?: AbortSignal) => {
    invalidateSqlCompletion();
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
  }, [client, invalidateSqlCompletion]);

  const refreshDrivers = useCallback(async (signal?: AbortSignal) => {
    const requestId = ++driverRequestRef.current;
    setDriversLoading(true);
    setDriversError(null);
    try {
      const inventory = await client.listDrivers(signal);
      if (!signal?.aborted && driverRequestRef.current === requestId) {
        setDrivers(inventory.items);
      }
    } catch (requestError) {
      if (
        !signal?.aborted
        && driverRequestRef.current === requestId
        && !(requestError instanceof DOMException && requestError.name === 'AbortError')
      ) {
        setDriversError(errorMessage(requestError));
      }
    } finally {
      if (!signal?.aborted && driverRequestRef.current === requestId) {
        setDriversLoading(false);
      }
    }
  }, [client]);

  useEffect(() => {
    const controller = new AbortController();
    void client.health(controller.signal).then(setHealth).catch((requestError: unknown) => {
      if (!(requestError instanceof DOMException && requestError.name === 'AbortError')) {
        setError(errorMessage(requestError));
      }
    });
    void refreshDrivers(controller.signal);
    void refreshDatasources(controller.signal);
    return () => controller.abort();
  }, [client, refreshDatasources, refreshDrivers]);

  useEffect(() => () => {
    activeOperationIdRef.current = null;
    previewRequestSequenceRef.current += 1;
    previewStartingRef.current = false;
    subscriptionRef.current?.close();
  }, []);

  useEffect(() => {
    invalidateSqlCompletion();
    formatRequestRef.current += 1;
    setFormatLoading(false);
    inspectionRequestRef.current += 1;
    setInspectionLoading(null);
    setSqlInspection(null);
    setCommunityParserAvailable(false);
  }, [invalidateSqlCompletion, selectedDatasourceKey]);

  useEffect(() => {
    if (sqlCompletion.status === 'success') {
      completionOptionRefs.current[sqlCompletion.selectedIndex]?.scrollIntoView({ block: 'nearest' });
    }
  }, [sqlCompletion]);

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
    if (activeOperationIdRef.current !== snapshot.operationId) return;
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
    if (activeOperationIdRef.current !== envelope.operationId) return;
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

  const clearOperationView = useCallback(() => {
    subscriptionRef.current?.close();
    subscriptionRef.current = null;
    activeOperationIdRef.current = null;
    resultRequestRef.current += 1;
    setResultLoading(false);
    setError(null);
    setResultPage(null);
    setOperation(null);
  }, []);

  const observeAcceptedOperation = useCallback((operationId: string) => {
    clearOperationView();
    activeOperationIdRef.current = operationId;
    setOperation({
      id: operationId,
      status: 'starting',
      rowCount: '0',
      byteCount: '0',
    });
    subscriptionRef.current = observeOperation(client, operationId, {
      onEvent: applyOperationEvent,
      onSnapshot: applySnapshot,
      onError: (streamError) => {
        if (activeOperationIdRef.current === operationId) setError(errorMessage(streamError));
      },
    });
  }, [applyOperationEvent, applySnapshot, clearOperationView, client]);

  const runQuery = async () => {
    if (!selectedDatasource || !sql.trim() || queryRunning || previewStartingRef.current) return;
    clearOperationView();
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
    } catch (requestError) {
      setError(errorMessage(requestError));
      setOperation(null);
      return;
    }
    observeAcceptedOperation(accepted.operationId);
  };

  const previewTable = useCallback(async (
    request: StartCommunityTablePreviewRequest,
    signal: AbortSignal,
  ) => {
    if (queryRunning || previewStartingRef.current || signal.aborted) return;
    const requestId = ++previewRequestSequenceRef.current;
    previewStartingRef.current = true;
    setPreviewStarting(true);
    setError(null);
    const invalidate = () => {
      if (previewRequestSequenceRef.current !== requestId) return;
      previewRequestSequenceRef.current += 1;
      previewStartingRef.current = false;
      setPreviewStarting(false);
    };
    signal.addEventListener('abort', invalidate, { once: true });
    try {
      const accepted = await client.startCommunityTablePreview(request, signal);
      if (signal.aborted || previewRequestSequenceRef.current !== requestId) {
        void client.cancelOperation(accepted.operationId).catch(() => undefined);
        return;
      }
      updateSql(accepted.sql);
      observeAcceptedOperation(accepted.operationId);
    } catch (requestError) {
      if (signal.aborted || previewRequestSequenceRef.current !== requestId) return;
      throw requestError;
    } finally {
      signal.removeEventListener('abort', invalidate);
      if (previewRequestSequenceRef.current === requestId) {
        previewStartingRef.current = false;
        setPreviewStarting(false);
      }
    }
  }, [client, observeAcceptedOperation, queryRunning, updateSql]);

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
        updateSql(formatted.sql);
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

  const completeSql = async () => {
    const editor = sqlEditorRef.current;
    if (
      !editor
      || !selectedDatasource?.hasSecret
      || !sql.trim()
      || !communityDatabaseType
      || communityCompatibility?.state !== 'ready'
      || sqlCompletion.status === 'loading'
    ) return;

    const cursorUtf16 = editor.selectionStart;
    editorCursorRef.current = cursorUtf16;
    setEditorCursorUtf16(cursorUtf16);
    const context = completionContextRef.current;
    const scope = {
      datasourceKey: selectedDatasourceKey,
      databaseType: communityDatabaseType,
      databaseName: context.databaseName,
      schemaName: context.schemaName,
      sql,
      cursorUtf16,
      refreshGeneration: context.refreshGeneration,
    };
    const request = {
      sequence: ++completionRequestRef.current,
      scope,
    };
    completionScopeRef.current = scope;
    setError(null);
    setSqlCompletion({ status: 'loading' });
    try {
      const completion = await client.completeCommunitySql({
        datasourceId: selectedDatasource.id,
        databaseType: communityDatabaseType,
        databaseName: context.databaseName,
        schemaName: context.schemaName,
        sql,
        cursorUtf16,
        minPrefixLength: 0,
        needFullName: false,
        keywordCase: 'UPPER',
      });
      if (!isCurrentSqlCompletionRequest(
        request,
        completionRequestRef.current,
        completionScopeRef.current,
      )) return;
      const status = completion.status.toLowerCase();
      if (status === 'success' && completion.candidates.length > 0) {
        completionOptionRefs.current = [];
        setSqlCompletion({ status: 'success', result: completion, selectedIndex: 0 });
      } else if (status === 'empty' || status === 'success') {
        setSqlCompletion({ status: 'empty', message: 'No suggestions at this cursor' });
      } else {
        setSqlCompletion({
          status: 'error',
          message: completion.reasonCode ?? `SQL completion ${status || 'failed'}`,
        });
      }
    } catch (requestError) {
      if (isCurrentSqlCompletionRequest(
        request,
        completionRequestRef.current,
        completionScopeRef.current,
      )) setSqlCompletion({ status: 'error', message: errorMessage(requestError) });
    }
  };

  const applyCompletionCandidate = (
    completion: CommunitySqlCompletion,
    candidate: CommunitySqlCompletionCandidate,
  ) => {
    const replacement = applySqlCompletion(
      sql,
      completion.replaceStartUtf16,
      completion.replaceEndUtf16,
      candidate,
    );
    if (!replacement) {
      setSqlCompletion({ status: 'error', message: 'The completion returned an invalid edit range' });
      return;
    }
    updateSql(replacement.sql, replacement.caret);
    requestAnimationFrame(() => {
      const editor = sqlEditorRef.current;
      if (!editor) return;
      editor.focus();
      editor.setSelectionRange(replacement.caret, replacement.caret);
    });
  };

  const closeSqlCompletion = () => {
    invalidateSqlCompletion();
    requestAnimationFrame(() => {
      const editor = sqlEditorRef.current;
      if (!editor) return;
      const caret = Math.min(editorCursorRef.current, editor.value.length);
      editor.focus();
      editor.setSelectionRange(caret, caret);
    });
  };

  const handleSqlEditorKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return;
    if ((event.ctrlKey || event.metaKey) && event.code === 'Space') {
      event.preventDefault();
      void completeSql();
      return;
    }
    if (event.key === 'Escape' && sqlCompletion.status !== 'idle') {
      event.preventDefault();
      closeSqlCompletion();
      return;
    }
    if (
      sqlCompletion.status === 'success'
      && (event.key === 'ArrowDown' || event.key === 'ArrowUp')
    ) {
      event.preventDefault();
      const direction = event.key === 'ArrowDown' ? 1 : -1;
      setSqlCompletion((current) => current.status === 'success'
        ? {
            ...current,
            selectedIndex: moveSqlCompletionSelection(
              current.selectedIndex,
              Math.min(current.result.candidates.length, MAX_VISIBLE_COMPLETION_CANDIDATES),
              direction,
            ),
          }
        : current);
      return;
    }
    if (sqlCompletion.status === 'success' && event.key === 'Enter') {
      const candidate = visibleCompletionCandidates[sqlCompletion.selectedIndex];
      if (!candidate) return;
      event.preventDefault();
      applyCompletionCandidate(sqlCompletion.result, candidate);
      return;
    }
    if (
      sqlCompletion.status !== 'idle'
      && !['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)
    ) {
      invalidateSqlCompletion();
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
        onCompletionContextChange={setCompletionContext}
        onInsertSql={updateSql}
        previewDisabled={queryRunning || previewStarting}
        onPreviewTable={previewTable}
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
                onClick={() => void completeSql()}
                disabled={
                  sqlCompletion.status === 'loading'
                  || !selectedDatasource?.hasSecret
                  || !sql.trim()
                  || !communityDatabaseType
                  || communityCompatibility?.state !== 'ready'
                }
              >
                {sqlCompletion.status === 'loading' ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <ListPlus size={16} aria-hidden="true" />}
                {sqlCompletion.status === 'loading' ? 'Completing' : 'Complete'}
              </button>
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
              <button className="primary-button" type="button" onClick={() => void runQuery()} disabled={!selectedDatasource || !selectedDatasource.hasSecret || !sql.trim() || queryRunning || previewStarting}>
                {queryRunning || previewStarting ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <Play size={16} fill="currentColor" aria-hidden="true" />}
                {queryRunning ? 'Running' : previewStarting ? 'Starting' : 'Run'}
              </button>
            </div>
          </div>
          <div className="sql-editor-surface">
            <textarea
              ref={sqlEditorRef}
              value={sql}
              onChange={(event) => updateSql(event.currentTarget.value, event.currentTarget.selectionStart)}
              onSelect={(event) => trackEditorCursor(
                event.currentTarget.selectionStart,
                event.currentTarget.value.length,
              )}
              onMouseDown={() => {
                if (sqlCompletion.status !== 'idle') invalidateSqlCompletion();
              }}
              onKeyDown={handleSqlEditorKeyDown}
              spellCheck={false}
              aria-label="SQL query"
              aria-autocomplete="list"
              aria-busy={sqlCompletion.status === 'loading'}
              aria-controls={sqlCompletion.status === 'success' ? 'sql-completion-listbox' : undefined}
              aria-expanded={sqlCompletion.status === 'success'}
              aria-activedescendant={sqlCompletion.status === 'success'
                ? `sql-completion-option-${sqlCompletion.selectedIndex}`
                : undefined}
            />
            {sqlCompletion.status !== 'idle' ? (
              <section className={`sql-completion-panel completion-${sqlCompletion.status}`} aria-live="polite">
                {sqlCompletion.status === 'loading' ? (
                  <div className="completion-message" role="status">
                    <LoaderCircle className="spinning" size={16} aria-hidden="true" />
                    <span>Loading suggestions</span>
                    <button className="completion-close" type="button" onMouseDown={(event) => event.preventDefault()} onClick={closeSqlCompletion} aria-label="Close suggestions" title="Close suggestions">
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                ) : null}
                {sqlCompletion.status === 'error' ? (
                  <div className="completion-message completion-error" role="alert">
                    <AlertCircle size={16} aria-hidden="true" />
                    <span>{sqlCompletion.message}</span>
                    <button className="completion-close" type="button" onMouseDown={(event) => event.preventDefault()} onClick={closeSqlCompletion} aria-label="Close suggestions" title="Close suggestions">
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                ) : null}
                {sqlCompletion.status === 'empty' ? (
                  <div className="completion-message" role="status">
                    <ListPlus size={16} aria-hidden="true" />
                    <span>{sqlCompletion.message}</span>
                    <button className="completion-close" type="button" onMouseDown={(event) => event.preventDefault()} onClick={closeSqlCompletion} aria-label="Close suggestions" title="Close suggestions">
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                ) : null}
                {sqlCompletion.status === 'success' ? (
                  <>
                    <header className="completion-heading">
                      <strong>Suggestions</strong>
                      <span>
                        {visibleCompletionCandidates.length}
                        {visibleCompletionCandidates.length < sqlCompletion.result.candidates.length
                          ? ` / ${sqlCompletion.result.candidates.length}`
                          : ''}
                      </span>
                      <button className="completion-close" type="button" onMouseDown={(event) => event.preventDefault()} onClick={closeSqlCompletion} aria-label="Close suggestions" title="Close suggestions">
                        <X size={14} aria-hidden="true" />
                      </button>
                    </header>
                    <div className="completion-options" id="sql-completion-listbox" role="listbox" aria-label="SQL completions">
                      {visibleCompletionCandidates.map((candidate, index) => (
                        <button
                          className="completion-option"
                          id={`sql-completion-option-${index}`}
                          type="button"
                          role="option"
                          tabIndex={-1}
                          aria-selected={index === sqlCompletion.selectedIndex}
                          key={`${candidate.id}:${index}`}
                          ref={(option) => { completionOptionRefs.current[index] = option; }}
                          onMouseDown={(event) => event.preventDefault()}
                          onClick={() => applyCompletionCandidate(sqlCompletion.result, candidate)}
                        >
                          <span className="completion-type" title={candidate.type}>{candidate.type}</span>
                          <span className="completion-copy">
                            <strong title={candidate.label}>{candidate.label}</strong>
                            <small title={candidate.detail || candidate.description || candidate.insertText || candidate.label}>
                              {candidate.detail || candidate.description || candidate.insertText || candidate.label}
                            </small>
                          </span>
                        </button>
                      ))}
                    </div>
                  </>
                ) : null}
              </section>
            ) : null}
          </div>
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
          drivers={drivers}
          driversLoading={driversLoading}
          driversError={driversError}
          busy={dialogBusy}
          submissionError={dialogError}
          onRetryDrivers={() => void refreshDrivers()}
          onClose={() => { if (!dialogBusy) { setDialog(null); setDialogError(null); } }}
          onSubmit={saveDatasource}
        />
      ) : null}
    </div>
  );
}

import {
  ChevronDown,
  ChevronRight,
  ChevronsDown,
  ChevronsUp,
  Code2,
  Database,
  Eye,
  FolderPlus,
  FolderTree,
  KeyRound,
  LoaderCircle,
  RefreshCw,
  Search,
  Sigma,
  Table2,
  Workflow,
  X,
  Zap,
} from 'lucide-react';
import {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

import type {
  BackendClient,
  CommunityDatabaseList,
  CommunityFunctionList,
  CommunityPluginCatalog,
  CommunityProcedureList,
  CommunitySchemaList,
  CommunityTableList,
  CommunityTriggerList,
  Datasource,
  HealthResponse,
  JdbcDriverList,
} from './backend';
import {
  CommunityFunctionDetail,
  CommunityLoadFailure,
  CommunityNamespaceSnapshot,
  CommunityProcedureDetail,
  CommunityTableDetail,
  CommunityTriggerDetail,
  loadCommunityFunctionDetail,
  loadCommunityNamespace,
  loadCommunityProcedureDetail,
  loadCommunityTableDetail,
  loadCommunityTriggerDetail,
  selectCommunityItem,
  selectCommunityPlugin,
} from './community-explorer-model';

type CommunityPlugin = CommunityPluginCatalog['plugins'][number];
type CommunityDatabase = CommunityDatabaseList['items'][number];
type CommunitySchema = CommunitySchemaList['items'][number];
type CommunityTable = CommunityTableList['items'][number];
type CommunityFunction = CommunityFunctionList['items'][number];
type CommunityProcedure = CommunityProcedureList['items'][number];
type CommunityTrigger = CommunityTriggerList['items'][number];
type InstalledDriver = JdbcDriverList['items'][number];
type CommunityHealth = HealthResponse['components'][number];

type ExplorerSelection =
  | { kind: 'table'; item: CommunityTable }
  | { kind: 'view'; item: CommunityTable }
  | { kind: 'function'; name: string; item?: CommunityFunction }
  | { kind: 'procedure'; name: string; item?: CommunityProcedure }
  | { kind: 'trigger'; name: string; item?: CommunityTrigger };

type ExplorerDetail =
  | { kind: 'table' | 'view'; value: CommunityTableDetail }
  | { kind: 'function'; value: CommunityFunctionDetail }
  | { kind: 'procedure'; value: CommunityProcedureDetail }
  | { kind: 'trigger'; value: CommunityTriggerDetail };

type DetailTab = 'columns' | 'indexes' | 'keys';
type LookupKind = 'function' | 'procedure' | 'trigger';

const EMPTY_NAMESPACE: CommunityNamespaceSnapshot = {
  tables: [],
  views: [],
  functions: [],
  procedures: [],
  triggers: [],
  failures: [],
};

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : 'Community metadata request failed';
}

function selectionName(selection: ExplorerSelection): string {
  return 'name' in selection ? selection.name : selection.item.name;
}

function selectionKey(selection: ExplorerSelection): string {
  return `${selection.kind}:${selectionName(selection)}`;
}

function groupNamedItems<T extends { name: string }>(items: readonly T[]) {
  const groups = new Map<string, { item: T; count: number }>();
  for (const item of items) {
    const group = groups.get(item.name);
    if (group) group.count += 1;
    else groups.set(item.name, { item, count: 1 });
  }
  return [...groups.values()];
}

function PartialWarning({ failures }: { failures: CommunityLoadFailure[] }) {
  if (failures.length === 0) return null;
  return (
    <div className="partial-warning" role="status">
      Unavailable: {failures.map((failure) => failure.area).join(', ')}
    </div>
  );
}

function ExplorerState({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="explorer-state">
      {icon}
      <span>{children}</span>
    </div>
  );
}

function ObjectGroup({
  label,
  icon,
  count,
  open,
  onToggle,
  children,
}: {
  label: string;
  icon: ReactNode;
  count: number;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section className="object-group">
      <button className="object-group-heading" type="button" onClick={onToggle} aria-expanded={open}>
        {open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
        {icon}
        <span>{label}</span>
        <small>{count}</small>
      </button>
      {open ? <div className="object-group-items">{children}</div> : null}
    </section>
  );
}

function EmptyItems() {
  return <span className="empty-object-group">None</span>;
}

function ObjectButton({
  active,
  icon,
  name,
  meta,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  name: string;
  meta?: string;
  onClick: () => void;
}) {
  return (
    <button
      className={`object-item ${active ? 'active' : ''}`}
      type="button"
      onClick={onClick}
      aria-current={active ? 'true' : undefined}
    >
      {icon}
      <span><strong>{name}</strong>{meta ? <small>{meta}</small> : null}</span>
    </button>
  );
}

function MetadataRows({
  rows,
}: {
  rows: Array<{ name: string; value?: string; badge?: string }>;
}) {
  if (rows.length === 0) return <div className="detail-empty">No metadata</div>;
  return (
    <div className="metadata-rows">
      {rows.map((row, index) => (
        <div className="metadata-row" key={`${row.name}-${index}`}>
          <span><strong>{row.name}</strong>{row.value ? <small>{row.value}</small> : null}</span>
          {row.badge ? <code>{row.badge}</code> : null}
        </div>
      ))}
    </div>
  );
}

function TableDetailView({ detail, tab, onTabChange }: {
  detail: CommunityTableDetail;
  tab: DetailTab;
  onTabChange: (tab: DetailTab) => void;
}) {
  const tabs: DetailTab[] = ['columns', 'indexes', 'keys'];
  const moveTab = (event: ReactKeyboardEvent<HTMLButtonElement>, item: DetailTab) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const direction = event.key === 'ArrowRight' ? 1 : -1;
    const nextIndex = (tabs.indexOf(item) + direction + tabs.length) % tabs.length;
    onTabChange(tabs[nextIndex]);
    const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
    buttons?.[nextIndex]?.focus();
  };
  return (
    <>
      <div className="detail-tabs" role="tablist" aria-label="Table metadata">
        {tabs.map((item) => (
          <button
            className={tab === item ? 'active' : ''}
            type="button"
            role="tab"
            aria-selected={tab === item}
            aria-controls="community-table-detail-panel"
            id={`community-table-detail-tab-${item}`}
            tabIndex={tab === item ? 0 : -1}
            onClick={() => onTabChange(item)}
            onKeyDown={(event) => moveTab(event, item)}
            key={item}
          >
            {item}
          </button>
        ))}
      </div>
      <div
        className="detail-content"
        id="community-table-detail-panel"
        role="tabpanel"
        aria-labelledby={`community-table-detail-tab-${tab}`}
      >
        <PartialWarning failures={detail.failures} />
        {tab === 'columns' ? (
          <MetadataRows rows={detail.columns.map((column) => ({
            name: column.name,
            value: column.columnType || 'Unknown type',
            badge: column.primaryKey ? 'PK' : (column.nullable === 0 ? 'NOT NULL' : undefined),
          }))} />
        ) : null}
        {tab === 'indexes' ? (
          <MetadataRows rows={detail.indexes.map((index) => ({
            name: index.name,
            value: index.columns.map((column) => column.columnName).filter(Boolean).join(', ') || index.indexType,
            badge: index.unique ? 'UNIQUE' : undefined,
          }))} />
        ) : null}
        {tab === 'keys' ? (
          <div className="key-groups">
            <div><span>Primary</span><MetadataRows rows={detail.primaryKeys.map((key) => ({ name: key.name || key.columnName, value: key.columnName, badge: 'PK' }))} /></div>
            <div><span>Imported</span><MetadataRows rows={detail.importedKeys.map((key) => ({ name: key.foreignKeyName || key.foreignColumnName, value: `${key.foreignColumnName} -> ${key.primaryTableName}.${key.primaryColumnName}`, badge: 'FK' }))} /></div>
            <div><span>Exported</span><MetadataRows rows={detail.exportedKeys.map((key) => ({ name: key.foreignKeyName || key.foreignColumnName, value: `${key.primaryColumnName} -> ${key.foreignTableName}.${key.foreignColumnName}`, badge: 'REF' }))} /></div>
          </div>
        ) : null}
      </div>
    </>
  );
}

function FunctionDetailView({ detail }: { detail: CommunityFunctionDetail }) {
  return (
    <div className="detail-content routine-detail">
      <PartialWarning failures={detail.failures} />
      <MetadataRows rows={detail.parameters.map((parameter) => ({
        name: parameter.columnName || `Parameter ${parameter.ordinalPosition ?? ''}`,
        value: parameter.typeName,
        badge: parameter.isNullable || undefined,
      }))} />
      {detail.function.body ? <pre>{detail.function.body}</pre> : null}
    </div>
  );
}

function ProcedureDetailView({ detail }: { detail: CommunityProcedureDetail }) {
  return (
    <div className="detail-content routine-detail">
      <PartialWarning failures={detail.failures} />
      <MetadataRows rows={detail.parameters.map((parameter) => ({
        name: parameter.columnName || `Parameter ${parameter.ordinalPosition ?? ''}`,
        value: parameter.typeName,
        badge: parameter.isNullable || undefined,
      }))} />
      {detail.procedure.body ? <pre>{detail.procedure.body}</pre> : null}
    </div>
  );
}

function TriggerDetailView({ detail }: { detail: CommunityTriggerDetail }) {
  return (
    <div className="detail-content routine-detail">
      <MetadataRows rows={detail.trigger.eventManipulation ? [{ name: 'Event', value: detail.trigger.eventManipulation }] : []} />
      {detail.trigger.body ? <pre>{detail.trigger.body}</pre> : null}
    </div>
  );
}

function SchemaSqlDialog({
  databaseName,
  busy,
  error,
  onClose,
  onBuild,
}: {
  databaseName: string;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onBuild: (value: { name: string; owner: string; comment: string }) => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [owner, setOwner] = useState('');
  const [comment, setComment] = useState('');
  const [validation, setValidation] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(
    typeof document === 'undefined' ? null : document.activeElement as HTMLElement | null,
  );

  useEffect(() => () => returnFocusRef.current?.focus(), []);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable?.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) {
      setValidation('Schema name is required.');
      return;
    }
    setValidation(null);
    await onBuild({ name: name.trim(), owner: owner.trim(), comment: comment.trim() });
  };

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        ref={dialogRef}
        className="dialog schema-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="schema-dialog-title"
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div><span className="section-kicker">{databaseName}</span><h2 id="schema-dialog-title">Create schema SQL</h2></div>
          <button className="icon-button quiet" type="button" onClick={onClose} aria-label="Close" title="Close"><X size={18} aria-hidden="true" /></button>
        </header>
        <form aria-busy={busy} onSubmit={(event) => void submit(event)}>
          <div className="form-grid two-columns">
            <label><span>Name</span><input autoFocus value={name} onChange={(event) => setName(event.target.value)} /></label>
            <label><span>Owner</span><input value={owner} onChange={(event) => setOwner(event.target.value)} /></label>
          </div>
          <label><span>Comment</span><input value={comment} onChange={(event) => setComment(event.target.value)} /></label>
          {validation || error ? <p className="form-error" role="alert">{validation ?? error}</p> : null}
          <footer className="dialog-actions">
            <button className="secondary-button" type="button" onClick={onClose}>Cancel</button>
            <button className="primary-button" type="submit" disabled={busy}>
              {busy ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <Code2 size={16} aria-hidden="true" />}
              Use SQL
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}

export interface CommunityExplorerProps {
  client: BackendClient;
  datasource?: Datasource;
  compatibility?: CommunityHealth;
  databaseType: string;
  onDatabaseTypeChange: (databaseType: string) => void;
  onParserAvailabilityChange: (available: boolean) => void;
  onCompletionContextChange: (context: CommunityCompletionContext) => void;
  onInsertSql: (sql: string) => void;
}

export interface CommunityCompletionContext {
  databaseName: string;
  schemaName: string;
  refreshGeneration: number;
}

export function CommunityExplorer({
  client,
  datasource,
  compatibility,
  databaseType,
  onDatabaseTypeChange,
  onParserAvailabilityChange,
  onCompletionContextChange,
  onInsertSql,
}: CommunityExplorerProps) {
  const [catalog, setCatalog] = useState<CommunityPluginCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [installedDrivers, setInstalledDrivers] = useState<InstalledDriver[]>([]);
  const [driversLoaded, setDriversLoaded] = useState(false);
  const [databases, setDatabases] = useState<CommunityDatabase[]>([]);
  const [databaseName, setDatabaseName] = useState('');
  const [schemas, setSchemas] = useState<CommunitySchema[]>([]);
  const [schemaName, setSchemaName] = useState('');
  const [namespace, setNamespace] = useState<CommunityNamespaceSnapshot>(EMPTY_NAMESPACE);
  const [selection, setSelection] = useState<ExplorerSelection | null>(null);
  const [detail, setDetail] = useState<ExplorerDetail | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>('columns');
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [loadingDatabases, setLoadingDatabases] = useState(false);
  const [loadingSchemas, setLoadingSchemas] = useState(false);
  const [loadingNamespace, setLoadingNamespace] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [refreshRevision, setRefreshRevision] = useState(0);
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({
    tables: true,
    views: true,
    functions: true,
    procedures: true,
    triggers: true,
  });
  const [lookupKind, setLookupKind] = useState<LookupKind>('function');
  const [lookupName, setLookupName] = useState('');
  const [schemaDialogOpen, setSchemaDialogOpen] = useState(false);
  const [schemaBusy, setSchemaBusy] = useState(false);
  const [schemaError, setSchemaError] = useState<string | null>(null);
  const [mobileCollapsed, setMobileCollapsed] = useState(
    () => typeof window !== 'undefined' && window.matchMedia('(max-width: 720px)').matches,
  );
  const schemaRequestRef = useRef<AbortController | null>(null);
  const databaseNameRef = useRef('');
  const schemaNameRef = useRef('');
  const databaseScopeRef = useRef('');
  const schemaScopeRef = useRef('');
  const namespaceScopeRef = useRef('');

  const plugins = useMemo(
    () => catalog?.plugins.filter((plugin) => plugin.services.metadataAvailable) ?? [],
    [catalog],
  );
  const selectedPlugin = plugins.find((plugin) => plugin.databaseType === databaseType);
  const groupedFunctions = useMemo(() => groupNamedItems(namespace.functions), [namespace.functions]);
  const groupedProcedures = useMemo(() => groupNamedItems(namespace.procedures), [namespace.procedures]);
  const activeSelectionKey = selection ? selectionKey(selection) : '';
  const ready = compatibility?.state === 'ready';
  const databaseScopeKey = ready && datasource?.hasSecret && driversLoaded && databaseType
    ? `${datasource.id}:${datasource.revision}:${databaseType}`
    : '';
  const schemaScopeKey = databaseScopeKey && databaseName
    ? `${databaseScopeKey}:${databaseName}`
    : '';
  const namespaceScopeKey = schemaScopeKey && schemaName
    ? `${schemaScopeKey}:${schemaName}`
    : '';
  const scopeReady = Boolean(namespaceScopeKey);
  const loadingScope = loadingDatabases || loadingSchemas;

  useEffect(() => () => schemaRequestRef.current?.abort(), []);

  useEffect(() => { databaseNameRef.current = databaseName; }, [databaseName]);
  useEffect(() => { schemaNameRef.current = schemaName; }, [schemaName]);

  useEffect(() => {
    onCompletionContextChange({
      databaseName,
      schemaName,
      refreshGeneration: refreshRevision,
    });
  }, [databaseName, onCompletionContextChange, refreshRevision, schemaName]);

  useEffect(() => {
    if (typeof window === 'undefined') return undefined;
    const media = window.matchMedia('(max-width: 720px)');
    const syncCollapsedState = () => setMobileCollapsed(media.matches);
    media.addEventListener('change', syncCollapsedState);
    return () => media.removeEventListener('change', syncCollapsedState);
  }, []);

  useEffect(() => {
    setCatalogError(null);
    setError(null);
    setLoadingCatalog(false);
    if (!ready) {
      setCatalog(null);
      setInstalledDrivers([]);
      setDriversLoaded(false);
      return undefined;
    }
    const controller = new AbortController();
    setLoadingCatalog(true);
    void Promise.allSettled([
      client.listCommunityPlugins(controller.signal),
      client.listDrivers(controller.signal),
    ]).then(([catalogResult, driversResult]) => {
      if (controller.signal.aborted) return;
      const failures: string[] = [];
      if (catalogResult.status === 'fulfilled') setCatalog(catalogResult.value);
      else if (!isAbortError(catalogResult.reason)) failures.push(messageFromError(catalogResult.reason));
      if (driversResult.status === 'fulfilled') {
        setInstalledDrivers(driversResult.value.items);
        setDriversLoaded(true);
      } else if (!isAbortError(driversResult.reason)) {
        failures.push(`Driver inventory: ${messageFromError(driversResult.reason)}`);
      }
      setCatalogError(failures.length ? failures.join(' ') : null);
    }).finally(() => {
      if (!controller.signal.aborted) setLoadingCatalog(false);
    });
    return () => controller.abort();
  }, [client, ready, refreshRevision]);

  useEffect(() => {
    if (!catalog || !driversLoaded || plugins.length === 0) return;
    const plugin = selectCommunityPlugin(
      plugins,
      installedDrivers,
      datasource?.driverId,
      databaseType,
    );
    if (plugin && plugin.databaseType !== databaseType) onDatabaseTypeChange(plugin.databaseType);
  }, [catalog, databaseType, datasource?.driverId, driversLoaded, installedDrivers, onDatabaseTypeChange, plugins]);

  useEffect(() => {
    onParserAvailabilityChange(Boolean(selectedPlugin?.services.sqlParserAvailable));
  }, [databaseType, onParserAvailabilityChange, selectedPlugin?.services.sqlParserAvailable]);

  useEffect(() => {
    const scopeChanged = databaseScopeRef.current !== databaseScopeKey;
    databaseScopeRef.current = databaseScopeKey;
    const currentDatabaseName = scopeChanged ? '' : databaseNameRef.current;
    if (scopeChanged) {
      databaseNameRef.current = '';
      onCompletionContextChange({ databaseName: '', schemaName: '', refreshGeneration: refreshRevision });
      setDatabases([]);
      setDatabaseName('');
      setSchemas([]);
      setSchemaName('');
      setNamespace(EMPTY_NAMESPACE);
      setSelection(null);
      setDetail(null);
    }
    setError(null);
    setLoadingDatabases(false);
    if (!databaseScopeKey || !datasource) return undefined;
    const controller = new AbortController();
    setLoadingDatabases(true);
    void client.listCommunityDatabases({ datasourceId: datasource.id, databaseType }, controller.signal).then((response) => {
      if (controller.signal.aborted) return;
      setDatabases(response.items);
      const nextDatabaseName = selectCommunityItem(response.items, currentDatabaseName)?.name ?? '';
      databaseNameRef.current = nextDatabaseName;
      onCompletionContextChange({
        databaseName: nextDatabaseName,
        schemaName: '',
        refreshGeneration: refreshRevision,
      });
      setDatabaseName(nextDatabaseName);
    }).catch((requestError: unknown) => {
      if (!isAbortError(requestError)) setError(messageFromError(requestError));
    }).finally(() => {
      if (!controller.signal.aborted) setLoadingDatabases(false);
    });
    return () => controller.abort();
  }, [client, databaseScopeKey, databaseType, datasource, onCompletionContextChange, refreshRevision]);

  useEffect(() => {
    const scopeChanged = schemaScopeRef.current !== schemaScopeKey;
    schemaScopeRef.current = schemaScopeKey;
    const currentSchemaName = scopeChanged ? '' : schemaNameRef.current;
    if (scopeChanged) {
      schemaNameRef.current = '';
      onCompletionContextChange({ databaseName, schemaName: '', refreshGeneration: refreshRevision });
      setSchemas([]);
      setSchemaName('');
      setNamespace(EMPTY_NAMESPACE);
      setSelection(null);
      setDetail(null);
    }
    setError(null);
    setLoadingSchemas(false);
    if (!schemaScopeKey || !datasource) return undefined;
    const controller = new AbortController();
    setLoadingSchemas(true);
    void client.listCommunitySchemas({
      datasourceId: datasource.id,
      databaseType,
      databaseName,
    }, controller.signal).then((response) => {
      if (controller.signal.aborted) return;
      setSchemas(response.items);
      const preferred = response.items.find((schema) => schema.name.toUpperCase() === 'APP');
      const current = response.items.find((schema) => schema.name === currentSchemaName);
      const nextSchemaName = (current ?? preferred ?? selectCommunityItem(response.items))?.name
        ?? '';
      schemaNameRef.current = nextSchemaName;
      onCompletionContextChange({
        databaseName,
        schemaName: nextSchemaName,
        refreshGeneration: refreshRevision,
      });
      setSchemaName(nextSchemaName);
    }).catch((requestError: unknown) => {
      if (!isAbortError(requestError)) setError(messageFromError(requestError));
    }).finally(() => {
      if (!controller.signal.aborted) setLoadingSchemas(false);
    });
    return () => controller.abort();
  }, [
    client,
    databaseName,
    databaseType,
    datasource,
    onCompletionContextChange,
    refreshRevision,
    schemaScopeKey,
  ]);

  useEffect(() => {
    const scopeChanged = namespaceScopeRef.current !== namespaceScopeKey;
    namespaceScopeRef.current = namespaceScopeKey;
    setNamespace(EMPTY_NAMESPACE);
    if (scopeChanged) {
      setSelection(null);
      setDetail(null);
    }
    setDetailError(null);
    setError(null);
    setLoadingNamespace(false);
    if (!namespaceScopeKey || !datasource) return undefined;
    const controller = new AbortController();
    setLoadingNamespace(true);
    void loadCommunityNamespace(client, {
      datasourceId: datasource.id,
      databaseType,
      databaseName,
      schemaName,
    }, controller.signal, (response) => {
      if (!controller.signal.aborted) setNamespace(response);
    }).then((response) => {
      if (!controller.signal.aborted) setNamespace(response);
    }).catch((requestError: unknown) => {
      if (!isAbortError(requestError)) setError(messageFromError(requestError));
    }).finally(() => {
      if (!controller.signal.aborted) setLoadingNamespace(false);
    });
    return () => controller.abort();
  }, [client, databaseName, databaseType, datasource, namespaceScopeKey, refreshRevision, schemaName]);

  useEffect(() => {
    setDetail(null);
    setDetailError(null);
    setDetailTab('columns');
    setLoadingDetail(false);
    if (!selection || !scopeReady || !datasource) return undefined;
    const controller = new AbortController();
    const scope = {
      datasourceId: datasource.id,
      databaseType,
      databaseName,
      schemaName,
    };
    setLoadingDetail(true);
    let request: Promise<ExplorerDetail>;
    if (selection.kind === 'table' || selection.kind === 'view') {
      const kind = selection.kind;
      request = loadCommunityTableDetail(
        client,
        { ...scope, tableName: selection.item.name },
        controller.signal,
        (value) => { if (!controller.signal.aborted) setDetail({ kind, value }); },
      )
        .then((value) => ({ kind, value }));
    } else if (selection.kind === 'function') {
      request = loadCommunityFunctionDetail(
        client,
        { ...scope, functionName: selection.name },
        controller.signal,
        (value) => { if (!controller.signal.aborted) setDetail({ kind: 'function', value }); },
      )
        .then((value) => ({ kind: 'function', value }));
    } else if (selection.kind === 'procedure') {
      request = loadCommunityProcedureDetail(
        client,
        { ...scope, procedureName: selection.name },
        controller.signal,
        (value) => { if (!controller.signal.aborted) setDetail({ kind: 'procedure', value }); },
      )
        .then((value) => ({ kind: 'procedure', value }));
    } else {
      request = loadCommunityTriggerDetail(client, { ...scope, triggerName: selection.name }, controller.signal)
        .then((value) => ({ kind: 'trigger', value }));
    }
    void request.then((response) => {
      if (!controller.signal.aborted) setDetail(response);
    }).catch((requestError: unknown) => {
      if (!isAbortError(requestError)) setDetailError(messageFromError(requestError));
    }).finally(() => {
      if (!controller.signal.aborted) setLoadingDetail(false);
    });
    return () => controller.abort();
  }, [client, databaseName, databaseType, datasource, refreshRevision, scopeReady, schemaName, selection]);

  const toggleGroup = (group: string) => {
    setOpenGroups((current) => ({ ...current, [group]: !current[group] }));
  };

  const selectDatabaseName = (nextDatabaseName: string) => {
    onCompletionContextChange({
      databaseName: nextDatabaseName,
      schemaName: '',
      refreshGeneration: refreshRevision,
    });
    setDatabaseName(nextDatabaseName);
  };

  const selectSchemaName = (nextSchemaName: string) => {
    onCompletionContextChange({
      databaseName,
      schemaName: nextSchemaName,
      refreshGeneration: refreshRevision,
    });
    setSchemaName(nextSchemaName);
  };

  const refreshObjects = () => {
    setCatalogError(null);
    setError(null);
    setDetailError(null);
    const nextRefreshRevision = refreshRevision + 1;
    onCompletionContextChange({
      databaseName,
      schemaName,
      refreshGeneration: nextRefreshRevision,
    });
    setRefreshRevision(nextRefreshRevision);
  };

  const submitLookup = (event: FormEvent) => {
    event.preventDefault();
    const name = lookupName.trim();
    if (!name) return;
    if (lookupKind === 'function') setSelection({ kind: 'function', name });
    if (lookupKind === 'procedure') setSelection({ kind: 'procedure', name });
    if (lookupKind === 'trigger') setSelection({ kind: 'trigger', name });
    setLookupName('');
  };

  const buildSchemaSql = async (value: { name: string; owner: string; comment: string }) => {
    if (!selectedPlugin || !databaseName) return;
    schemaRequestRef.current?.abort();
    const controller = new AbortController();
    schemaRequestRef.current = controller;
    setSchemaBusy(true);
    setSchemaError(null);
    try {
      const response = await client.buildCommunityCreateSchema({
        databaseType: selectedPlugin.databaseType,
        schema: {
          databaseName,
          name: value.name,
          owner: value.owner,
          comment: value.comment,
          system: false,
        },
      }, controller.signal);
      if (controller.signal.aborted) return;
      onInsertSql(response.sql);
      setSchemaDialogOpen(false);
    } catch (requestError) {
      if (!isAbortError(requestError)) setSchemaError(messageFromError(requestError));
    } finally {
      if (schemaRequestRef.current === controller) {
        schemaRequestRef.current = null;
        setSchemaBusy(false);
      }
    }
  };

  const closeSchemaDialog = () => {
    schemaRequestRef.current?.abort();
    schemaRequestRef.current = null;
    setSchemaBusy(false);
    setSchemaDialogOpen(false);
    setSchemaError(null);
  };

  const panelBody = (() => {
    if (!compatibility) return <ExplorerState icon={<LoaderCircle className="spinning" size={20} />}>Checking compatibility</ExplorerState>;
    if (!ready) return <ExplorerState icon={<FolderTree size={22} />}>{compatibility.detail}</ExplorerState>;
    if (loadingCatalog && (!catalog || !driversLoaded)) return <ExplorerState icon={<LoaderCircle className="spinning" size={20} />}>Loading plugins</ExplorerState>;
    if (!catalog) return <ExplorerState icon={<FolderTree size={22} />}>Community catalog unavailable</ExplorerState>;
    if (!datasource) return <ExplorerState icon={<Database size={22} />}>Select a datasource</ExplorerState>;
    if (!datasource.hasSecret) return <ExplorerState icon={<KeyRound size={22} />}>Connection details required</ExplorerState>;
    if (!driversLoaded) return <ExplorerState icon={<Database size={22} />}>Driver inventory unavailable</ExplorerState>;
    if (catalog && plugins.length === 0) return <ExplorerState icon={<FolderTree size={22} />}>No metadata plugins</ExplorerState>;
    return null;
  })();
  const displayedError = catalogError ?? error;

  return (
    <aside className={`community-explorer ${mobileCollapsed ? 'mobile-collapsed' : ''}`} aria-label="Database objects">
      <header className="explorer-heading">
        <div><span className="section-kicker">Community</span><h1>Objects</h1></div>
        <div className="explorer-actions">
          <button
            className="icon-button compact-button explorer-collapse"
            type="button"
            onClick={() => setMobileCollapsed((collapsed) => !collapsed)}
            aria-expanded={!mobileCollapsed}
            aria-label={mobileCollapsed ? 'Show database objects' : 'Hide database objects'}
            title={mobileCollapsed ? 'Show database objects' : 'Hide database objects'}
          >{mobileCollapsed ? <ChevronsDown size={15} aria-hidden="true" /> : <ChevronsUp size={15} aria-hidden="true" />}</button>
          <button
            className="icon-button compact-button"
            type="button"
            onClick={() => { setSchemaError(null); setSchemaDialogOpen(true); }}
            disabled={!selectedPlugin?.services.sqlBuilderAvailable || !databaseName}
            aria-label="Create schema SQL"
            title="Create schema SQL"
          ><FolderPlus size={16} aria-hidden="true" /></button>
          <button
            className="icon-button compact-button"
            type="button"
            onClick={refreshObjects}
            disabled={!ready}
            aria-label="Retry and refresh objects"
            title="Retry and refresh objects"
          ><RefreshCw className={loadingCatalog || loadingScope || loadingNamespace ? 'spinning' : undefined} size={15} aria-hidden="true" /></button>
        </div>
      </header>

      {displayedError ? <div className="explorer-error" role="alert"><span>{displayedError}</span><button type="button" onClick={() => { setCatalogError(null); setError(null); }} aria-label="Dismiss error" title="Dismiss error"><X size={14} /></button></div> : null}
      {panelBody ?? (
        <>
          <div className="explorer-selectors">
            <label><span>Plugin</span><select value={databaseType} onChange={(event) => onDatabaseTypeChange(event.target.value)}>{databaseType ? null : <option value="" disabled>Select plugin</option>}{plugins.map((plugin) => <option value={plugin.databaseType} key={plugin.databaseType}>{plugin.name}</option>)}</select></label>
            <label><span>Database</span><select value={databaseName} onChange={(event) => selectDatabaseName(event.target.value)} disabled={loadingScope || databases.length === 0}>{databases.map((database) => <option value={database.name} key={database.name}>{database.name}</option>)}</select></label>
            <label><span>Schema</span><select value={schemaName} onChange={(event) => selectSchemaName(event.target.value)} disabled={loadingScope || schemas.length === 0}>{schemas.map((schema) => <option value={schema.name} key={`${schema.databaseName}:${schema.name}`}>{schema.name}</option>)}</select></label>
          </div>

          <div className="object-tree" aria-busy={loadingNamespace}>
            {loadingNamespace && !namespace.tables.length && !namespace.views.length ? <ExplorerState icon={<LoaderCircle className="spinning" size={18} />}>Loading objects</ExplorerState> : null}
            {!loadingNamespace && !schemaName ? <ExplorerState icon={<FolderTree size={20} />}>No schema selected</ExplorerState> : null}
            {schemaName ? (
              <>
                <PartialWarning failures={namespace.failures} />
                <ObjectGroup label="Tables" icon={<Table2 size={14} />} count={namespace.tables.length} open={openGroups.tables} onToggle={() => toggleGroup('tables')}>
                  {namespace.tables.length ? namespace.tables.map((table) => <ObjectButton key={`${table.databaseName}:${table.schemaName}:${table.name}`} active={activeSelectionKey === `table:${table.name}`} icon={<Table2 size={14} />} name={table.name} meta={table.tableType} onClick={() => setSelection({ kind: 'table', item: table })} />) : <EmptyItems />}
                </ObjectGroup>
                <ObjectGroup label="Views" icon={<Eye size={14} />} count={namespace.views.length} open={openGroups.views} onToggle={() => toggleGroup('views')}>
                  {namespace.views.length ? namespace.views.map((view) => <ObjectButton key={`${view.databaseName}:${view.schemaName}:${view.name}`} active={activeSelectionKey === `view:${view.name}`} icon={<Eye size={14} />} name={view.name} onClick={() => setSelection({ kind: 'view', item: view })} />) : <EmptyItems />}
                </ObjectGroup>
                <ObjectGroup label="Functions" icon={<Sigma size={14} />} count={groupedFunctions.length} open={openGroups.functions} onToggle={() => toggleGroup('functions')}>
                  {groupedFunctions.length ? groupedFunctions.map(({ item: fn, count }) => <ObjectButton key={`${fn.databaseName}:${fn.schemaName}:${fn.name}`} active={activeSelectionKey === `function:${fn.name}`} icon={<Sigma size={14} />} name={fn.name} meta={count > 1 ? `${count} overloads` : fn.specificName} onClick={() => setSelection({ kind: 'function', name: fn.name, item: fn })} />) : <EmptyItems />}
                </ObjectGroup>
                <ObjectGroup label="Procedures" icon={<Workflow size={14} />} count={groupedProcedures.length} open={openGroups.procedures} onToggle={() => toggleGroup('procedures')}>
                  {groupedProcedures.length ? groupedProcedures.map(({ item: procedure, count }) => <ObjectButton key={`${procedure.databaseName}:${procedure.schemaName}:${procedure.name}`} active={activeSelectionKey === `procedure:${procedure.name}`} icon={<Workflow size={14} />} name={procedure.name} meta={count > 1 ? `${count} overloads` : procedure.specificName} onClick={() => setSelection({ kind: 'procedure', name: procedure.name, item: procedure })} />) : <EmptyItems />}
                </ObjectGroup>
                <ObjectGroup label="Triggers" icon={<Zap size={14} />} count={namespace.triggers.length} open={openGroups.triggers} onToggle={() => toggleGroup('triggers')}>
                  {namespace.triggers.length ? namespace.triggers.map((trigger) => <ObjectButton key={`${trigger.databaseName}:${trigger.schemaName}:${trigger.name}`} active={activeSelectionKey === `trigger:${trigger.name}`} icon={<Zap size={14} />} name={trigger.name} meta={trigger.eventManipulation} onClick={() => setSelection({ kind: 'trigger', name: trigger.name, item: trigger })} />) : <EmptyItems />}
                </ObjectGroup>
                <form className="direct-lookup" onSubmit={submitLookup}>
                  <select aria-label="Object type" value={lookupKind} onChange={(event) => setLookupKind(event.target.value as LookupKind)}><option value="function">Function</option><option value="procedure">Procedure</option><option value="trigger">Trigger</option></select>
                  <input aria-label="Object name" value={lookupName} onChange={(event) => setLookupName(event.target.value)} placeholder="Open by name" spellCheck={false} />
                  <button type="submit" disabled={!lookupName.trim()} aria-label="Open object" title="Open object"><Search size={14} aria-hidden="true" /></button>
                </form>
              </>
            ) : null}
          </div>

          <section className="object-detail" aria-label="Object details" aria-busy={loadingDetail}>
            <header>
              <div><span className="section-kicker">Details</span><strong>{selection ? selectionName(selection) : 'No selection'}</strong></div>
              {selection ? <code>{selection.kind}</code> : null}
            </header>
            {loadingDetail && !detail ? <ExplorerState icon={<LoaderCircle className="spinning" size={18} />}>Loading details</ExplorerState> : null}
            {detailError ? <div className="detail-error" role="alert">{detailError}</div> : null}
            {!loadingDetail && !detailError && !detail ? <ExplorerState icon={<FolderTree size={20} />}>Select an object</ExplorerState> : null}
            {loadingDetail && detail ? <div className="partial-warning" role="status">Loading remaining metadata</div> : null}
            {detail?.kind === 'table' ? <TableDetailView detail={detail.value} tab={detailTab} onTabChange={setDetailTab} /> : null}
            {detail?.kind === 'view' ? <TableDetailView detail={detail.value} tab={detailTab} onTabChange={setDetailTab} /> : null}
            {detail?.kind === 'function' ? <FunctionDetailView detail={detail.value} /> : null}
            {detail?.kind === 'procedure' ? <ProcedureDetailView detail={detail.value} /> : null}
            {detail?.kind === 'trigger' ? <TriggerDetailView detail={detail.value} /> : null}
          </section>
        </>
      )}

      {schemaDialogOpen ? (
        <SchemaSqlDialog
          databaseName={databaseName}
          busy={schemaBusy}
          error={schemaError}
          onClose={closeSchemaDialog}
          onBuild={buildSchemaSql}
        />
      ) : null}
    </aside>
  );
}

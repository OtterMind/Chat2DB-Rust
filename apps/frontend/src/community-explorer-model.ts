import type {
  BackendClient,
  CommunityForeignKeyList,
  CommunityFunction,
  CommunityFunctionList,
  CommunityFunctionParameterList,
  CommunityPluginCatalog,
  CommunityPrimaryKeyList,
  CommunityProcedure,
  CommunityProcedureList,
  CommunityProcedureParameterList,
  CommunityTableColumnList,
  CommunityTableIndexList,
  CommunityTableList,
  CommunityTrigger,
  CommunityTriggerList,
  CommunityViewList,
  JdbcDriverList,
} from './backend/client';

export type CommunityLoadArea =
  | 'tables'
  | 'views'
  | 'functions'
  | 'procedures'
  | 'triggers'
  | 'columns'
  | 'indexes'
  | 'importedKeys'
  | 'exportedKeys'
  | 'primaryKeys'
  | 'functionParameters'
  | 'procedureParameters';

export interface CommunityLoadFailure {
  area: CommunityLoadArea;
  error: unknown;
}

export interface CommunityNamespaceRequest {
  datasourceId: string;
  databaseType: string;
  databaseName: string;
  schemaName: string;
}

export interface CommunityTableDetailRequest extends CommunityNamespaceRequest {
  tableName: string;
}

export interface CommunityFunctionDetailRequest extends CommunityNamespaceRequest {
  functionName: string;
}

export interface CommunityProcedureDetailRequest extends CommunityNamespaceRequest {
  procedureName: string;
}

export interface CommunityTriggerDetailRequest extends CommunityNamespaceRequest {
  triggerName: string;
}

export interface CommunityNamespaceSnapshot {
  tables: CommunityTableList['items'];
  views: CommunityViewList['items'];
  functions: CommunityFunctionList['items'];
  procedures: CommunityProcedureList['items'];
  triggers: CommunityTriggerList['items'];
  failures: CommunityLoadFailure[];
}

export interface CommunityTableDetail {
  columns: CommunityTableColumnList['items'];
  indexes: CommunityTableIndexList['items'];
  importedKeys: CommunityForeignKeyList['items'];
  exportedKeys: CommunityForeignKeyList['items'];
  primaryKeys: CommunityPrimaryKeyList['items'];
  failures: CommunityLoadFailure[];
}

export interface CommunityFunctionDetail {
  function: CommunityFunction;
  parameters: CommunityFunctionParameterList['items'];
  failures: CommunityLoadFailure[];
}

export interface CommunityProcedureDetail {
  procedure: CommunityProcedure;
  parameters: CommunityProcedureParameterList['items'];
  failures: CommunityLoadFailure[];
}

export interface CommunityTriggerDetail {
  trigger: CommunityTrigger;
}

export type CommunityNamespaceClient = Pick<
  BackendClient,
  | 'listCommunityTables'
  | 'listCommunityViews'
  | 'listCommunityFunctions'
  | 'listCommunityProcedures'
  | 'listCommunityTriggers'
>;

export type CommunityTableDetailClient = Pick<
  BackendClient,
  | 'listCommunityColumns'
  | 'listCommunityIndexes'
  | 'listCommunityImportedKeys'
  | 'listCommunityExportedKeys'
  | 'listCommunityPrimaryKeys'
>;

export type CommunityFunctionDetailClient = Pick<
  BackendClient,
  'getCommunityFunction' | 'listCommunityFunctionParameters'
>;

export type CommunityProcedureDetailClient = Pick<
  BackendClient,
  'getCommunityProcedure' | 'listCommunityProcedureParameters'
>;

export type CommunityTriggerDetailClient = Pick<BackendClient, 'getCommunityTrigger'>;

async function settleItems<T>(
  area: CommunityLoadArea,
  request: Promise<{ items: T[] }>,
  onSuccess: (items: T[]) => void,
  failures: Map<CommunityLoadArea, unknown>,
  onSettled: () => void,
): Promise<void> {
  try {
    onSuccess((await request).items);
  } catch (error) {
    failures.set(area, error);
  } finally {
    onSettled();
  }
}

export async function loadCommunityNamespace(
  client: CommunityNamespaceClient,
  request: CommunityNamespaceRequest,
  signal?: AbortSignal,
  onProgress?: (snapshot: CommunityNamespaceSnapshot) => void,
): Promise<CommunityNamespaceSnapshot> {
  const scope = {
    datasourceId: request.datasourceId,
    databaseType: request.databaseType,
    databaseName: request.databaseName,
    schemaName: request.schemaName,
  };
  let tables: CommunityNamespaceSnapshot['tables'] = [];
  let views: CommunityNamespaceSnapshot['views'] = [];
  let functions: CommunityNamespaceSnapshot['functions'] = [];
  let procedures: CommunityNamespaceSnapshot['procedures'] = [];
  let triggers: CommunityNamespaceSnapshot['triggers'] = [];
  const areas: CommunityLoadArea[] = ['tables', 'views', 'functions', 'procedures', 'triggers'];
  const failures = new Map<CommunityLoadArea, unknown>();
  const snapshot = (): CommunityNamespaceSnapshot => ({
    tables,
    views,
    functions,
    procedures,
    triggers,
    failures: areas.flatMap((area) => (
      failures.has(area) ? [{ area, error: failures.get(area) }] : []
    )),
  });
  const publish = () => {
    if (!signal?.aborted) onProgress?.(snapshot());
  };

  await Promise.all([
    settleItems('tables', client.listCommunityTables({ ...scope, tableNamePattern: '%' }, signal), (items) => { tables = items; }, failures, publish),
    settleItems('views', client.listCommunityViews({ ...scope, viewNamePattern: '%' }, signal), (items) => { views = items; }, failures, publish),
    settleItems('functions', client.listCommunityFunctions({ ...scope }, signal), (items) => { functions = items; }, failures, publish),
    settleItems('procedures', client.listCommunityProcedures({ ...scope }, signal), (items) => { procedures = items; }, failures, publish),
    settleItems('triggers', client.listCommunityTriggers({ ...scope }, signal), (items) => { triggers = items; }, failures, publish),
  ]);

  return snapshot();
}

export async function loadCommunityTableDetail(
  client: CommunityTableDetailClient,
  request: CommunityTableDetailRequest,
  signal?: AbortSignal,
  onProgress?: (detail: CommunityTableDetail) => void,
): Promise<CommunityTableDetail> {
  const detailRequest = {
    datasourceId: request.datasourceId,
    databaseType: request.databaseType,
    databaseName: request.databaseName,
    schemaName: request.schemaName,
    tableName: request.tableName,
  };
  let columns: CommunityTableDetail['columns'] = [];
  let indexes: CommunityTableDetail['indexes'] = [];
  let importedKeys: CommunityTableDetail['importedKeys'] = [];
  let exportedKeys: CommunityTableDetail['exportedKeys'] = [];
  let primaryKeys: CommunityTableDetail['primaryKeys'] = [];
  const areas: CommunityLoadArea[] = ['columns', 'indexes', 'importedKeys', 'exportedKeys', 'primaryKeys'];
  const failures = new Map<CommunityLoadArea, unknown>();
  const snapshot = (): CommunityTableDetail => ({
    columns,
    indexes,
    importedKeys,
    exportedKeys,
    primaryKeys,
    failures: areas.flatMap((area) => (
      failures.has(area) ? [{ area, error: failures.get(area) }] : []
    )),
  });
  const publish = () => {
    if (!signal?.aborted) onProgress?.(snapshot());
  };

  await Promise.all([
    settleItems('columns', client.listCommunityColumns({ ...detailRequest }, signal), (items) => { columns = items; }, failures, publish),
    settleItems('indexes', client.listCommunityIndexes({ ...detailRequest }, signal), (items) => { indexes = items; }, failures, publish),
    settleItems('importedKeys', client.listCommunityImportedKeys({ ...detailRequest }, signal), (items) => { importedKeys = items; }, failures, publish),
    settleItems('exportedKeys', client.listCommunityExportedKeys({ ...detailRequest }, signal), (items) => { exportedKeys = items; }, failures, publish),
    settleItems('primaryKeys', client.listCommunityPrimaryKeys({ ...detailRequest }, signal), (items) => { primaryKeys = items; }, failures, publish),
  ]);

  return snapshot();
}

export async function loadCommunityFunctionDetail(
  client: CommunityFunctionDetailClient,
  request: CommunityFunctionDetailRequest,
  signal?: AbortSignal,
  onProgress?: (detail: CommunityFunctionDetail) => void,
): Promise<CommunityFunctionDetail> {
  const detailRequest = {
    datasourceId: request.datasourceId,
    databaseType: request.databaseType,
    databaseName: request.databaseName,
    schemaName: request.schemaName,
    functionName: request.functionName,
  };
  let detail: CommunityFunction | undefined;
  let parameters: CommunityFunctionDetail['parameters'] = [];
  let parametersFailed = false;
  let parameterFailure: unknown;
  const snapshot = (): CommunityFunctionDetail | undefined => detail ? {
    function: detail,
    parameters,
    failures: !parametersFailed
      ? []
      : [{ area: 'functionParameters', error: parameterFailure }],
  } : undefined;
  const publish = () => {
    const current = snapshot();
    if (current && !signal?.aborted) onProgress?.(current);
  };
  const parameterRequest = client.listCommunityFunctionParameters({ ...detailRequest }, signal)
    .then((response) => { parameters = response.items; })
    .catch((error: unknown) => { parametersFailed = true; parameterFailure = error; })
    .finally(publish);

  detail = await client.getCommunityFunction({ ...detailRequest }, signal);
  publish();
  await parameterRequest;
  return snapshot() as CommunityFunctionDetail;
}

export async function loadCommunityProcedureDetail(
  client: CommunityProcedureDetailClient,
  request: CommunityProcedureDetailRequest,
  signal?: AbortSignal,
  onProgress?: (detail: CommunityProcedureDetail) => void,
): Promise<CommunityProcedureDetail> {
  const detailRequest = {
    datasourceId: request.datasourceId,
    databaseType: request.databaseType,
    databaseName: request.databaseName,
    schemaName: request.schemaName,
    procedureName: request.procedureName,
  };
  let detail: CommunityProcedure | undefined;
  let parameters: CommunityProcedureDetail['parameters'] = [];
  let parametersFailed = false;
  let parameterFailure: unknown;
  const snapshot = (): CommunityProcedureDetail | undefined => detail ? {
    procedure: detail,
    parameters,
    failures: !parametersFailed
      ? []
      : [{ area: 'procedureParameters', error: parameterFailure }],
  } : undefined;
  const publish = () => {
    const current = snapshot();
    if (current && !signal?.aborted) onProgress?.(current);
  };
  const parameterRequest = client.listCommunityProcedureParameters({ ...detailRequest }, signal)
    .then((response) => { parameters = response.items; })
    .catch((error: unknown) => { parametersFailed = true; parameterFailure = error; })
    .finally(publish);

  detail = await client.getCommunityProcedure({ ...detailRequest }, signal);
  publish();
  await parameterRequest;
  return snapshot() as CommunityProcedureDetail;
}

export async function loadCommunityTriggerDetail(
  client: CommunityTriggerDetailClient,
  request: CommunityTriggerDetailRequest,
  signal?: AbortSignal,
): Promise<CommunityTriggerDetail> {
  const trigger = await client.getCommunityTrigger({ ...request }, signal);
  return { trigger };
}

export interface CommunitySelectionItem {
  id?: string;
  name: string;
  system?: boolean;
}

export function selectCommunityPlugin(
  plugins: CommunityPluginCatalog['plugins'],
  installedDrivers: JdbcDriverList['items'],
  datasourceDriverId?: string,
  currentDatabaseType?: string,
): CommunityPluginCatalog['plugins'][number] | undefined {
  const current = plugins.find((plugin) => plugin.databaseType === currentDatabaseType);
  if (current) return current;
  const installedDriver = installedDrivers.find((driver) => driver.driverId === datasourceDriverId);
  const driverMatch = installedDriver
    ? plugins.find((plugin) => plugin.drivers.some(
      (driver) => driver.jdbcDriverClass === installedDriver.driverClass,
    ))
    : undefined;
  return driverMatch ?? (datasourceDriverId ? undefined : plugins[0]);
}

export function selectCommunityItem<T extends CommunitySelectionItem>(
  items: readonly T[],
  currentIdOrName?: string | null,
): T | undefined {
  const current = currentIdOrName == null
    ? undefined
    : items.find((item) => item.id === currentIdOrName || item.name === currentIdOrName);
  return current ?? items.find((item) => item.system !== true) ?? items[0];
}

/**
 * On-demand Java runtime helpers for the desktop build. The runtime archive
 * carries the JDBC engine and driver packs that compatibility databases need.
 */
export interface JavaRuntimeStatus {
  installed: boolean;
  version?: string | null;
  path?: string | null;
}

interface TauriEvent {
  payload?: unknown;
}

interface TauriGlobal {
  core?: {
    invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
  event?: {
    listen?: (event: string, handler: (event: TauriEvent) => void) => Promise<() => void>;
  };
}

function tauri(): TauriGlobal | undefined {
  return (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__;
}

/** Whether the page runs inside the desktop shell rather than a browser. */
export function desktopRuntimeAvailable(): boolean {
  return typeof tauri()?.core?.invoke === 'function';
}

/** Reads the installed state; browser builds simply report nothing. */
export async function javaRuntimeStatus(): Promise<JavaRuntimeStatus | null> {
  const invoke = tauri()?.core?.invoke;
  if (!invoke) {
    return null;
  }
  try {
    return (await invoke('java_runtime_status')) as JavaRuntimeStatus;
  } catch {
    return null;
  }
}

/** Downloads and unpacks the runtime archive, reporting download progress. */
export async function installJavaRuntime(
  onProgress?: (progress: { downloadedBytes: number; totalBytes: number | null }) => void,
): Promise<void> {
  const api = tauri();
  const invoke = api?.core?.invoke;
  if (!invoke) {
    throw new Error('desktop runtime is unavailable');
  }
  let unlisten: (() => void) | undefined;
  if (onProgress && typeof api?.event?.listen === 'function') {
    unlisten = await api.event.listen('chat2db://java-runtime-install-progress', (event) => {
      const payload = (event.payload ?? {}) as {
        downloadedBytes?: number;
        totalBytes?: number | null;
      };
      onProgress({
        downloadedBytes: payload.downloadedBytes ?? 0,
        totalBytes: payload.totalBytes ?? null,
      });
    });
  }
  try {
    await invoke('java_runtime_install');
  } finally {
    unlisten?.();
  }
}

/** Downloads one database's JDBC driver pack into the installed runtime. */
export async function installJdbcDriverPack(
  databaseType: string,
  onProgress?: (progress: { downloadedBytes: number; totalBytes: number | null }) => void,
): Promise<void> {
  const api = tauri();
  const invoke = api?.core?.invoke;
  if (!invoke) {
    throw new Error('desktop runtime is unavailable');
  }
  let unlisten: (() => void) | undefined;
  if (onProgress && typeof api?.event?.listen === 'function') {
    unlisten = await api.event.listen('chat2db://java-runtime-install-progress', (event) => {
      const payload = (event.payload ?? {}) as {
        downloadedBytes?: number;
        totalBytes?: number | null;
      };
      onProgress({
        downloadedBytes: payload.downloadedBytes ?? 0,
        totalBytes: payload.totalBytes ?? null,
      });
    });
  }
  try {
    await invoke('jdbc_driver_pack_install', { databaseType });
  } finally {
    unlisten?.();
  }
}

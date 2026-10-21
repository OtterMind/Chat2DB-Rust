export type RedisKeyViewMode = 'list' | 'tree';

interface RedisViewModeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function createRedisKeyViewModeStorageKey(clientEdition: string, runtimeEnv: string) {
  return `chat2db.${clientEdition}.${runtimeEnv}.redis.key-view-mode.v1`;
}

export function getRedisViewModeStorage(): RedisViewModeStorage | undefined {
  try {
    return typeof window === 'undefined' ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

export function readRedisKeyViewMode(
  storage: RedisViewModeStorage | undefined,
  storageKey: string,
): RedisKeyViewMode {
  try {
    const storedValue = storage?.getItem(storageKey);
    return storedValue === 'tree' || storedValue === 'list' ? storedValue : 'list';
  } catch {
    return 'list';
  }
}

export function persistRedisKeyViewMode(
  storage: RedisViewModeStorage | undefined,
  storageKey: string,
  viewMode: RedisKeyViewMode,
) {
  try {
    storage?.setItem(storageKey, viewMode);
  } catch {
    // Storage can be unavailable in restricted browser contexts.
  }
}

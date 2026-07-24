import { isTauri } from '@tauri-apps/api/core';

import type { BackendClient } from './client';
import { HttpBackendClient } from './http';
import { TauriBackendClient } from './tauri';

export * from './client';
export * from './operation';
export { HttpBackendClient } from './http';
export { TauriBackendClient } from './tauri';

export function createBackendClient(): BackendClient {
  return isTauri() ? new TauriBackendClient() : new HttpBackendClient();
}

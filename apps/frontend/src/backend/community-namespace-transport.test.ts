import { describe, expect, it, vi } from 'vitest';

import type {
  BuildCommunityNamespaceSqlRequest,
  CommunityBuiltSql,
} from './client';
import { HttpBackendClient } from './http';
import { TauriBackendClient } from './tauri';

const request = {
  databaseType: 'POSTGRESQL',
  operation: {
    kind: 'alterSchema',
    oldSchemaName: 'app',
    newSchemaName: 'reporting',
  },
} satisfies BuildCommunityNamespaceSqlRequest;

describe('Community namespace transport cancellation', () => {
  it('forwards the HTTP abort signal with the unwrapped request body', async () => {
    const fetch = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => (
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => {
          reject(init.signal?.reason ?? new DOMException('Aborted', 'AbortError'));
        }, { once: true });
      })
    ));
    const controller = new AbortController();
    const client = new HttpBackendClient({ baseUrl: 'http://127.0.0.1:4200', fetch });

    const pending = client.buildCommunityNamespaceSql(request, controller.signal);

    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:4200/api/v1/community/sql/build-namespace',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(request),
        signal: controller.signal,
      }),
    );

    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('stops awaiting a late Tauri response while preserving its request wrapper', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    let resolveInvoke: ((value: CommunityBuiltSql) => void) | undefined;
    const invoke = <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return new Promise<CommunityBuiltSql>((resolve) => {
        resolveInvoke = resolve;
      }) as Promise<T>;
    };
    const controller = new AbortController();
    const client = new TauriBackendClient({ invoke });

    const pending = client.buildCommunityNamespaceSql(request, controller.signal);
    expect(calls).toEqual([{
      command: 'build_community_namespace_sql',
      args: { request },
    }]);

    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });

    resolveInvoke?.({ sql: 'ALTER SCHEMA app RENAME TO reporting' });
    await Promise.resolve();
  });
});

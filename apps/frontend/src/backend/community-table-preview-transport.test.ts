import { describe, expect, it, vi } from 'vitest';

import type {
  CommunityTablePreviewAccepted,
  StartCommunityTablePreviewRequest,
} from './client';
import { HttpBackendClient } from './http';
import { TauriBackendClient } from './tauri';

const request = {
  datasourceId: 'source-1',
  databaseType: 'MYSQL',
  databaseName: 'inventory',
  schemaName: '',
  tableName: 'items',
} satisfies StartCommunityTablePreviewRequest;

const accepted = {
  operationId: 'operation-preview-1',
  sql: 'SELECT * FROM `inventory`.`items` LIMIT 200',
  rowLimit: 200,
} satisfies CommunityTablePreviewAccepted;

describe('Community table preview transport', () => {
  it('posts the unwrapped request to the HTTP 202 endpoint with its abort signal', async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify(accepted), {
      status: 202,
      headers: { 'Content-Type': 'application/json' },
    }));
    const controller = new AbortController();
    const client = new HttpBackendClient({ baseUrl: 'http://127.0.0.1:4200', fetch });

    await expect(client.startCommunityTablePreview(request, controller.signal)).resolves.toEqual(accepted);
    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:4200/api/v1/community/table-preview',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(request),
        signal: controller.signal,
      }),
    );
  });

  it('stops awaiting a late Tauri response while preserving its request wrapper', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    let resolveInvoke: ((value: CommunityTablePreviewAccepted) => void) | undefined;
    const invoke = <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return new Promise<CommunityTablePreviewAccepted>((resolve) => {
        resolveInvoke = resolve;
      }) as Promise<T>;
    };
    const controller = new AbortController();
    const client = new TauriBackendClient({ invoke });

    const pending = client.startCommunityTablePreview(request, controller.signal);
    expect(calls).toEqual([{
      command: 'start_community_table_preview',
      args: { request },
    }]);

    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });

    resolveInvoke?.(accepted);
    await Promise.resolve();
  });
});

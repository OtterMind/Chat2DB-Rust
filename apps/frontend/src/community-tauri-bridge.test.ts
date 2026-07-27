import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

import { describe, expect, it, vi } from 'vitest';

const bridgeSource = readFileSync(
  new URL('../../../scripts/community-tauri-bridge.js', import.meta.url),
  'utf8',
);

describe('Community Tauri bridge', () => {
  it('does not alter the Community Web runtime', () => {
    const window = {};
    runInNewContext(bridgeSource, { globalThis: { window } });
    expect(window).not.toHaveProperty('javaQuery');
  });

  it('forwards the existing javaQuery contract through one Tauri command', async () => {
    const invoke = vi.fn().mockResolvedValue('{"ok":true}');
    const onSuccess = vi.fn();
    const onFailure = vi.fn();
    const window: Record<string, unknown> = {};
    const globalObject = {
      __TAURI__: { core: { invoke } },
      window,
    };

    runInNewContext(bridgeSource, { globalThis: globalObject });
    const javaQuery = window.javaQuery as (request: Record<string, unknown>) => void;
    javaQuery({ request: '{"uuid":"request-1"}', onSuccess, onFailure });
    await Promise.resolve();

    expect(invoke).toHaveBeenCalledWith('legacy_request', {
      request: '{"uuid":"request-1"}',
    });
    expect(onSuccess).toHaveBeenCalledWith('{"ok":true}');
    expect(onFailure).not.toHaveBeenCalled();
  });
});

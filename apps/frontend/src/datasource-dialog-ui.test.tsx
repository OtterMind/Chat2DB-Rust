// @vitest-environment happy-dom

import { act } from 'react';
import { createRoot, Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BackendClient, HealthResponse, JdbcDriver } from './backend';
import App, { DatasourceDialog } from './App';

vi.mock('./CommunityExplorer', () => ({ CommunityExplorer: () => null }));

const mysql = {
  packId: 'mysql',
  name: 'MySQL',
  version: '8.0.30',
  driverId: 'sha256:mysql',
  driverClass: 'com.mysql.cj.jdbc.Driver',
  artifactCount: 1,
  artifactBytes: '2513563',
} satisfies JdbcDriver;

const postgresql = {
  ...mysql,
  packId: 'postgresql',
  name: 'PostgreSQL',
  version: '42.7.7',
  driverId: 'sha256:postgresql',
  driverClass: 'org.postgresql.Driver',
} satisfies JdbcDriver;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true;
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  document.body.replaceChildren();
});

async function setInput(input: HTMLInputElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  await act(async () => {
    setter?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
}

async function waitFor<T>(read: () => T | null | undefined): Promise<T> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const value = read();
    if (value !== null && value !== undefined) return value;
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  }
  throw new Error('Timed out waiting for the datasource UI state');
}

const health = {
  components: [],
  product: { edition: 'community', name: 'Chat2DB', version: '0.1.0' },
  status: 'degraded',
  uptimeSeconds: 1,
} satisfies HealthResponse;

describe('Datasource driver picker', () => {
  it('submits an installed driver without exposing its opaque ID as an input', async () => {
    const onSubmit = vi.fn(async () => undefined);
    await act(async () => {
      root.render(
        <DatasourceDialog
          dialog={{ kind: 'create' }}
          drivers={[mysql, postgresql]}
          driversLoading={false}
          driversError={null}
          busy={false}
          submissionError={null}
          onRetryDrivers={vi.fn()}
          onClose={vi.fn()}
          onSubmit={onSubmit}
        />,
      );
    });

    const driver = container.querySelector<HTMLSelectElement>('select[aria-label="Driver"]');
    expect(driver).not.toBeNull();
    expect([...driver!.options].map((option) => option.textContent)).toEqual([
      'MySQL 8.0.30 \u00b7 com.mysql.cj.jdbc.Driver',
      'PostgreSQL 42.7.7 \u00b7 org.postgresql.Driver',
    ]);
    expect(driver!.value).toBe(mysql.driverId);
    expect(container.querySelector('input[value="sha256:mysql"]')).toBeNull();
    expect(container.textContent).not.toContain('Driver ID');

    await setInput(
      container.querySelector<HTMLInputElement>('input[aria-label="Datasource name"]')!,
      'Local MySQL',
    );
    await setInput(
      container.querySelector<HTMLInputElement>('input[aria-label="JDBC URL"]')!,
      'jdbc:mysql://127.0.0.1:3306/app',
    );
    const form = container.querySelector<HTMLFormElement>('form')!;
    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await Promise.resolve();
    });

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Local MySQL',
      driverId: mysql.driverId,
      jdbcUrl: 'jdbc:mysql://127.0.0.1:3306/app',
    }));
  });

  it('shows a recoverable error when the driver inventory cannot be loaded', async () => {
    const onRetryDrivers = vi.fn();
    await act(async () => {
      root.render(
        <DatasourceDialog
          dialog={{ kind: 'create' }}
          drivers={[]}
          driversLoading={false}
          driversError="No verified JDBC drivers are loaded."
          busy={false}
          submissionError={null}
          onRetryDrivers={onRetryDrivers}
          onClose={vi.fn()}
          onSubmit={vi.fn(async () => undefined)}
        />,
      );
    });

    expect(container.querySelector<HTMLSelectElement>('select[aria-label="Driver"]')?.disabled).toBe(true);
    expect(container.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled).toBe(true);
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      'Could not load installed drivers: No verified JDBC drivers are loaded.',
    );

    const retry = [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === 'Retry');
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(onRetryDrivers).toHaveBeenCalledOnce();
  });

  it('keeps an unavailable current driver visible and editable when inventory loading fails', async () => {
    const onSubmit = vi.fn(async () => undefined);
    await act(async () => {
      root.render(
        <DatasourceDialog
          dialog={{
            kind: 'edit',
            datasource: {
              id: 'source-1',
              name: 'Legacy MySQL',
              driverId: 'sha256:legacy-mysql',
              hasSecret: true,
              revision: '7',
              createdAtMs: '1',
              updatedAtMs: '2',
            },
          }}
          drivers={[]}
          driversLoading={false}
          driversError="driver service unavailable"
          busy={false}
          submissionError={null}
          onRetryDrivers={vi.fn()}
          onClose={vi.fn()}
          onSubmit={onSubmit}
        />,
      );
    });

    const driver = container.querySelector<HTMLSelectElement>('select[aria-label="Driver"]')!;
    expect(driver.disabled).toBe(false);
    expect(driver.value).toBe('sha256:legacy-mysql');
    expect(driver.selectedOptions[0]?.textContent).toBe(
      'Current driver (not installed) \u00b7 sha256:legacy-mysql',
    );
    expect(container.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled).toBe(false);
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      'The existing driver remains available for this edit.',
    );

    const form = container.querySelector<HTMLFormElement>('form')!;
    await act(async () => {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await Promise.resolve();
    });
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Legacy MySQL',
      driverId: 'sha256:legacy-mysql',
      connectionMode: 'keep',
    }));
  });

  it('reloads the App driver inventory after a failed initial request', async () => {
    const listDrivers = vi.fn()
      .mockRejectedValueOnce(new Error('inventory offline'))
      .mockResolvedValueOnce({ items: [mysql] });
    const client = {
      transport: 'http',
      health: vi.fn(async () => health),
      listDrivers,
      listDatasources: vi.fn(async () => ({ items: [] })),
    } as unknown as BackendClient;

    await act(async () => root.render(<App client={client} />));
    await act(async () => {
      container.querySelector<HTMLButtonElement>('button[aria-label="New datasource"]')!.click();
    });
    const retry = await waitFor(() => [...container.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === 'Retry'));

    await act(async () => retry.click());
    const driver = await waitFor(() => {
      const select = container.querySelector<HTMLSelectElement>('select[aria-label="Driver"]');
      return select?.value === mysql.driverId ? select : null;
    });

    expect(listDrivers).toHaveBeenCalledTimes(2);
    expect(driver.selectedOptions[0]?.textContent).toBe(
      'MySQL 8.0.30 \u00b7 com.mysql.cj.jdbc.Driver',
    );
  });
});

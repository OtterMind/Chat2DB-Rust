export type RuntimeStatus = 'ready' | 'degraded' | 'unavailable';
export type ComponentState = 'ready' | 'disabled' | 'unavailable';

export interface ProductInfo {
  name: string;
  version: string;
  edition: string;
}

export interface ComponentHealth {
  id: string;
  label: string;
  state: ComponentState;
  detail: string;
}

export interface HealthResponse {
  product: ProductInfo;
  status: RuntimeStatus;
  uptimeSeconds: number;
  components: ComponentHealth[];
}

const REQUEST_TIMEOUT_MS = 10_000;
const runtimeStatuses: ReadonlySet<RuntimeStatus> = new Set([
  'ready',
  'degraded',
  'unavailable',
]);
const componentStates: ReadonlySet<ComponentState> = new Set([
  'ready',
  'disabled',
  'unavailable',
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isProductInfo(value: unknown): value is ProductInfo {
  return isRecord(value)
    && typeof value.name === 'string'
    && typeof value.version === 'string'
    && typeof value.edition === 'string';
}

function isComponentHealth(value: unknown): value is ComponentHealth {
  return isRecord(value)
    && typeof value.id === 'string'
    && typeof value.label === 'string'
    && typeof value.state === 'string'
    && componentStates.has(value.state as ComponentState)
    && typeof value.detail === 'string';
}

export function isHealthResponse(value: unknown): value is HealthResponse {
  return isRecord(value)
    && isProductInfo(value.product)
    && typeof value.status === 'string'
    && runtimeStatuses.has(value.status as RuntimeStatus)
    && typeof value.uptimeSeconds === 'number'
    && Number.isFinite(value.uptimeSeconds)
    && value.uptimeSeconds >= 0
    && Array.isArray(value.components)
    && value.components.every(isComponentHealth);
}

export async function getRuntimeHealth(signal?: AbortSignal): Promise<HealthResponse> {
  const controller = new AbortController();
  const timeoutError = new Error('Runtime health request timed out');
  const timeout = setTimeout(() => controller.abort(timeoutError), REQUEST_TIMEOUT_MS);
  const abortFromCaller = () => controller.abort(signal?.reason);

  if (signal?.aborted) {
    abortFromCaller();
  } else {
    signal?.addEventListener('abort', abortFromCaller, { once: true });
  }

  try {
    const response = await fetch('/api/v1/system/health', {
      headers: { Accept: 'application/json' },
      signal: controller.signal,
    });

    if (!response.ok) {
      throw new Error(`Runtime health request failed with HTTP ${response.status}`);
    }

    const payload: unknown = await response.json();
    if (!isHealthResponse(payload)) {
      throw new Error('Runtime health response does not match the application contract');
    }

    return payload;
  } catch (requestError) {
    if (controller.signal.reason === timeoutError) throw timeoutError;
    throw requestError;
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener('abort', abortFromCaller);
  }
}

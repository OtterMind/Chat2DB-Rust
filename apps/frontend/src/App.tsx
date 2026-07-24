import {
  Bot,
  CircleGauge,
  Database,
  HardDrive,
  Plug,
  RefreshCw,
  Settings,
  TerminalSquare,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { ComponentHealth, HealthResponse, getRuntimeHealth } from './api';

const componentIcons = {
  'ai-agent': Bot,
  'database-engine': Database,
  'local-storage': HardDrive,
  'product-core': CircleGauge,
};

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function ComponentTile({ component }: { component: ComponentHealth }) {
  const Icon = componentIcons[component.id as keyof typeof componentIcons] ?? Plug;

  return (
    <article className="component-tile">
      <div className={`component-icon state-${component.state}`}>
        <Icon size={18} aria-hidden="true" />
      </div>
      <div className="component-copy">
        <div className="component-heading">
          <h2>{component.label}</h2>
          <span className={`status-label state-${component.state}`}>{component.state}</span>
        </div>
        <p>{component.detail}</p>
      </div>
    </article>
  );
}

export default function App() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    try {
      setHealth(await getRuntimeHealth(signal));
      setError(null);
    } catch (requestError) {
      if (requestError instanceof DOMException && requestError.name === 'AbortError') return;
      setHealth(null);
      setError(requestError instanceof Error ? requestError.message : 'Runtime is unavailable');
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    return () => controller.abort();
  }, [refresh]);

  const readyCount = useMemo(
    () => health?.components.filter((component) => component.state === 'ready').length ?? 0,
    [health],
  );

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark"><Database size={20} aria-hidden="true" /></span>
          <span>Chat2DB</span>
        </div>
        <nav aria-label="Primary navigation">
          <a className="nav-item active" href="#runtime" aria-current="page">
            <CircleGauge size={18} aria-hidden="true" />
            <span>Runtime</span>
          </a>
          <span className="nav-item disabled" aria-disabled="true">
            <Plug size={18} aria-hidden="true" />
            <span>Connections</span>
          </span>
          <span className="nav-item disabled" aria-disabled="true">
            <Bot size={18} aria-hidden="true" />
            <span>AI Agent</span>
          </span>
        </nav>
        <div className="sidebar-footer">
          <span className="nav-item disabled" aria-disabled="true">
            <Settings size={18} aria-hidden="true" />
            <span>Settings</span>
          </span>
        </div>
      </aside>

      <main className="workspace" id="runtime">
        <header className="topbar">
          <div>
            <p className="eyebrow">Community runtime</p>
            <h1>System status</h1>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
            aria-label="Refresh runtime status"
            title="Refresh runtime status"
          >
            <RefreshCw className={loading ? 'spinning' : undefined} size={18} aria-hidden="true" />
          </button>
        </header>

        {error ? (
          <section className="error-band" role="alert">
            <TerminalSquare size={18} aria-hidden="true" />
            <div>
              <strong>Runtime unavailable</strong>
              <span>{error}</span>
            </div>
          </section>
        ) : null}

        <section
          className="summary-band"
          aria-label="Runtime summary"
          aria-live="polite"
          aria-busy={loading}
        >
          <div>
            <span className={`summary-indicator status-${health?.status ?? 'unavailable'}`} />
            <div>
              <strong>{health?.status ?? (loading ? 'loading' : 'unavailable')}</strong>
              <span>{health ? `${readyCount} of ${health.components.length} components ready` : 'No runtime data'}</span>
            </div>
          </div>
          <dl>
            <div>
              <dt>Version</dt>
              <dd>{health?.product.version ?? '-'}</dd>
            </div>
            <div>
              <dt>Edition</dt>
              <dd>{health?.product.edition ?? '-'}</dd>
            </div>
            <div>
              <dt>Uptime</dt>
              <dd>{health ? formatUptime(health.uptimeSeconds) : '-'}</dd>
            </div>
          </dl>
        </section>

        <section className="component-grid" aria-label="Runtime components">
          {health?.components.map((component) => (
            <ComponentTile component={component} key={component.id} />
          ))}
        </section>
      </main>
    </div>
  );
}

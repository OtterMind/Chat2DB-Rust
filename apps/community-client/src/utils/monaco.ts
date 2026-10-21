let monacoEnvironmentInitialized = false;
let monacoCancellationRejectionHandlerInitialized = false;

const workerProxyUrls = new Map<string, string>();

const getWorkerFile = (label: string) => {
  if (label === 'json') {
    return 'json.worker.js';
  }

  return 'editor.worker.js';
};

const createWorkerProxyUrl = (workerUrl: string) => {
  const cached = workerProxyUrls.get(workerUrl);
  if (cached) {
    return cached;
  }

  const blob = new Blob(
    [
      `self.MonacoEnvironment = { baseUrl: ${JSON.stringify(new URL('./', window.location.href).href)} };`,
      `importScripts(${JSON.stringify(workerUrl)});`,
    ],
    { type: 'text/javascript' },
  );
  const proxyUrl = URL.createObjectURL(blob);
  workerProxyUrls.set(workerUrl, proxyUrl);
  return proxyUrl;
};

const isCancellationError = (reason: unknown) => {
  if (!reason || typeof reason !== 'object') {
    return false;
  }

  const error = reason as { name?: unknown; message?: unknown };
  return error.name === 'Canceled' || error.message === 'Canceled';
};

const isLikelyMonacoCancellationError = (reason: unknown) => {
  if (!isCancellationError(reason)) {
    return false;
  }

  const error = reason as { stack?: unknown };
  const stack = typeof error.stack === 'string' ? error.stack : '';
  return [
    'Delayer.cancel',
    'WordHighlighter',
    'WordHighlighterContribution',
    'StandaloneEditor.dispose',
    'CodeEditorContributions.dispose',
    'DisposableStore.dispose',
    'vs/editor/',
    'monaco-editor',
  ].some((marker) => stack.includes(marker));
};

const setupMonacoCancellationRejectionHandler = () => {
  if (monacoCancellationRejectionHandlerInitialized || typeof window === 'undefined') {
    return;
  }
  monacoCancellationRejectionHandlerInitialized = true;

  window.addEventListener(
    'unhandledrejection',
    (event) => {
      if (isLikelyMonacoCancellationError(event.reason)) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );
};

export const setupMonacoEnvironment = () => {
  setupMonacoCancellationRejectionHandler();

  if (monacoEnvironmentInitialized) {
    return;
  }
  monacoEnvironmentInitialized = true;

  if (!window.location.href.startsWith('file://')) {
    return;
  }

  const globalWindow = window as Window & {
    MonacoEnvironment?: {
      getWorker?: (_moduleId: string, label: string) => Worker;
    };
  };

  globalWindow.MonacoEnvironment = {
    ...globalWindow.MonacoEnvironment,
    getWorker: (_moduleId: string, label: string) => {
      const workerUrl = new URL(`./${getWorkerFile(label)}`, window.location.href).href;
      const proxyUrl = createWorkerProxyUrl(workerUrl);
      return new Worker(proxyUrl);
    },
  };
};

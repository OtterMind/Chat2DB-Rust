(() => {
  const tauri = globalThis.__TAURI__;
  const invoke = tauri?.core?.invoke;
  if (typeof invoke !== 'function' || typeof globalThis.window !== 'object') return;

  const listen = tauri?.event?.listen;
  if (typeof listen === 'function') {
    listen('chat2db://java-message', ({ payload }) => {
      if (typeof globalThis.window.handleJavaMessage !== 'function') return;
      const message = typeof payload === 'string' ? payload : JSON.stringify(payload);
      globalThis.window.handleJavaMessage(message);
    }).catch((error) => {
      console.error('Unable to subscribe to Chat2DB desktop events', error);
    });
  }

  if (globalThis.window.location?.hash === '') {
    globalThis.window.location.replace('#/workspace');
  }

  globalThis.window.javaQuery = ({ request, onSuccess, onFailure }) => {
    invoke('legacy_request', { request })
      .then((response) => onSuccess?.(response))
      .catch((error) => onFailure?.('legacy_request_failed', String(error)));
  };
})();

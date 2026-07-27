(() => {
  const invoke = globalThis.__TAURI__?.core?.invoke;
  if (typeof invoke !== 'function' || typeof globalThis.window !== 'object') return;

  globalThis.window.javaQuery = ({ request, onSuccess, onFailure }) => {
    invoke('legacy_request', { request })
      .then((response) => onSuccess?.(response))
      .catch((error) => onFailure?.('legacy_request_failed', String(error)));
  };
})();

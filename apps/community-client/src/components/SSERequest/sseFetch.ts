export interface SSEFetchMiddlewares {
  onRequest?: (...ags: Parameters<typeof fetch>) => Promise<Parameters<typeof fetch>>;
  onResponse?: (response: Response) => Promise<Response>;
}

export interface SSEFetchOptions extends globalThis.RequestInit {
  /**
   * @description A typeof fetch function
   * @default globalThis.fetch
   */
  fetch?: typeof fetch;
  /**
   * @description Middleware for request and response
   */
  middlewares?: SSEFetchMiddlewares;
}

export type SSEFetchType = (
  baseURL: Parameters<typeof fetch>[0],
  options?: SSEFetchOptions,
) => Promise<Response>;

const SSEFetch: SSEFetchType = async (baseURL, options = {}) => {
  const { fetch: fetchFn = globalThis.fetch, middlewares = {}, ...requestInit } = options;

  if (typeof fetchFn !== 'function') {
    throw new TypeError('The options.fetch must be a typeof fetch function!');
  }

  /** ---------------------- request init ---------------------- */
  let fetchArgs: Parameters<typeof fetch> = [baseURL, requestInit];

  /** ---------------------- request middleware ---------------------- */
  if (typeof middlewares.onRequest === 'function') {
    const modifiedFetchArgs = await middlewares.onRequest(...fetchArgs);

    fetchArgs = modifiedFetchArgs;
  }

  /** ---------------------- fetch ---------------------- */
  let response = await fetchFn(...fetchArgs);

  /** ---------------------- response middleware ---------------------- */
  if (typeof middlewares.onResponse === 'function') {
    const modifiedResponse = await middlewares.onResponse(response);

    if (!(modifiedResponse instanceof Response)) {
      throw new TypeError('The options.onResponse must return a Response instance!');
    }

    response = modifiedResponse;
  }

  /** ---------------------- response check ---------------------- */
  if (!response.ok) {
    throw new TypeError(`Fetch failed with status ${response.status}`);
  }

  if (!response.body) {
    throw new TypeError('The response body is empty.');
  }

  /** ---------------------- return ---------------------- */
  return response;
};

export default SSEFetch;

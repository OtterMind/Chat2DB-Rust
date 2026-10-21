import sseStream, { SSEOutput, SSEStreamProps } from './sseStream';
import sseFetch from './sseFetch';
import { SSERequestOptions, SSERequestParams, SSERequestCallbacks, AnyObject } from './index';
import { handleSSEErrorPayload } from './errorPayload';

class HTTPSRequestClass {
  readonly baseURL;
  readonly model;
  private defaultHeaders;
  private abortController?: AbortController;

  private static instanceBuffer: Map<string | typeof fetch, HTTPSRequestClass> = new Map();

  private constructor(options: SSERequestOptions) {
    const { baseURL, model } = options;

    this.baseURL = baseURL;
    this.model = model;
    this.defaultHeaders = {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream, application/json',
      'Cache-Control': 'no-cache',
      ...(options.dangerouslyApiKey && {
        Authorization: options.dangerouslyApiKey,
      }),
      'Accept-Language': options.lang || 'en-US',
    };
  }

  public static init(options: SSERequestOptions): HTTPSRequestClass {
    if (!options.baseURL || typeof options.baseURL !== 'string') {
      throw new Error('The baseURL is not valid!');
    }

    const id = options.baseURL;

    if (!HTTPSRequestClass.instanceBuffer.has(id)) {
      HTTPSRequestClass.instanceBuffer.set(id, new HTTPSRequestClass(options));
    }

    return HTTPSRequestClass.instanceBuffer.get(id)!;
  }

  public create = async <Input = AnyObject, Output = SSEOutput>(
    params: SSERequestParams & Input,
    callbacks: SSERequestCallbacks<Output>,
    transformStream?: SSEStreamProps<Output>['transformStream'],
  ) => {
    this.abortController = new AbortController();
    const requestInit = {
      method: 'POST',
      headers: this.defaultHeaders,
      body: JSON.stringify({
        model: this.model,
        ...params,
      }),
      signal: this.abortController.signal,
    };

    try {
      const response = await sseFetch(this.baseURL, requestInit);

      if (transformStream) {
        await this.customResponseHandler<Output>(response, callbacks, transformStream);
        return;
      }

      const contentType = response.headers.get('content-type') || '';

      const mimeType = contentType.split(';')[0].trim();
      switch (mimeType) {
        /** SSE */
        case 'text/event-stream': {
          await this.sseResponseHandler<Output>(response, callbacks, params);
          break;
        }

        /** JSON */
        case 'application/json': {
          await this.jsonResponseHandler<Output>(response, callbacks, params);
          break;
        }

        default: {
          throw new Error(`The response content-type: ${contentType} is not support!`);
        }
      }
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        callbacks?.onStop?.();
        return;
      }
      const err = error instanceof Error ? error : new Error('Unknown error!');
      callbacks?.onError?.(err);
      throw err;
    }
  };

  private customResponseHandler = async <Output = SSEOutput>(
    response: Response,
    callbacks?: SSERequestCallbacks<Output>,
    transformStream?: SSEStreamProps<Output>['transformStream'],
  ) => {
    const chunks: Output[] = [];

    for await (const chunk of sseStream({
      readableStream: response.body!,
      transformStream,
    })) {
      chunks.push(chunk);

      callbacks?.onUpdate?.(chunk);
    }

    callbacks?.onSuccess?.(chunks);
  };

  private sseResponseHandler = async <Output = SSEOutput>(
    response: Response,
    callbacks?: SSERequestCallbacks<Output>,
    requestParams?: AnyObject,
  ) => {
    const chunks: Output[] = [];
    for await (const chunk of sseStream<Output>({
      readableStream: response.body!,
    })) {
      if (handleSSEErrorPayload(chunk, requestParams)) {
        callbacks?.onSuccess?.(chunks);
        return;
      }

      if (chunk.data === '[DONE]') {
        callbacks?.onSuccess?.(chunks);
        return;
      }

      chunks.push(chunk);
      callbacks?.onUpdate?.(chunk);
    }

    callbacks?.onSuccess?.(chunks);
  };

  private jsonResponseHandler = async <Output = SSEOutput>(
    response: Response,
    callbacks?: SSERequestCallbacks<Output>,
    requestParams?: AnyObject,
  ) => {
    const chunk: Output = await response.json();

    if (handleSSEErrorPayload(chunk, requestParams)) {
      callbacks?.onSuccess?.([]);
      return;
    }

    callbacks?.onUpdate?.(chunk);

    callbacks?.onSuccess?.([chunk]);
  };

  public stop() {
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = undefined;
    }
  }
}

export default HTTPSRequestClass;

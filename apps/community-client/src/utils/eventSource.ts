import { EventSourcePolyfill } from 'event-source-polyfill';

const connectToEventSource = (params: {
  url: string;
  onOpen: () => void;
  onMessage: (data: string) => void;
  onError: (error: Event) => void;
}) => {
  const { url, onOpen, onMessage, onError } = params;

  if (!url || !onMessage || !onError) {
    throw new Error('url, onMessage, and onError are required');
  }

  // const eventSource = new EventSourcePolyfill(`${url}`, p);
  const eventSource = new EventSourcePolyfill(`${url}`);

  eventSource.onopen = () => {
    onOpen();
  };

  eventSource.onmessage = (event) => {
    // console.log('event', event);
    onMessage(event.data);
  };

  eventSource.onerror = (error) => {
    onError(error);
    console.error('EventSourcePolyfill error:', error);
  };

  // Returns a function that closes the eventSource so that it can be called when needed
  return () => {
    eventSource.close();
  };
};

export default connectToEventSource;

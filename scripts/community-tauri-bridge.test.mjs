import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

const source = await readFile(new URL('./community-tauri-bridge.js', import.meta.url), 'utf8');
let eventListener;
const requests = [];
const window = {
  location: {
    hash: '#/workspace',
    replace() {
      throw new Error('the existing route must not be replaced');
    },
  },
};
const context = {
  console,
  window,
  __TAURI__: {
    core: {
      invoke(command, payload) {
        requests.push({ command, payload });
        return Promise.resolve('{"ok":true}');
      },
    },
    event: {
      listen(eventName, listener) {
        assert.equal(eventName, 'chat2db://java-message');
        eventListener = listener;
        return Promise.resolve(() => {});
      },
    },
  },
};
context.globalThis = context;

vm.runInNewContext(source, context, { filename: 'community-tauri-bridge.js' });
assert.equal(typeof window.javaQuery, 'function');
assert.equal(typeof eventListener, 'function');

let response;
await new Promise((resolve, reject) => {
  window.javaQuery({
    request: '{"requestUrl":"/api/system","method":"get"}',
    onSuccess(value) {
      response = value;
      resolve();
    },
    onFailure: reject,
  });
});
assert.equal(response, '{"ok":true}');
assert.equal(
  JSON.stringify(requests),
  JSON.stringify([
    {
      command: 'legacy_request',
      payload: { request: '{"requestUrl":"/api/system","method":"get"}' },
    },
  ]),
);

let pushedMessage;
window.handleJavaMessage = (message) => {
  pushedMessage = message;
};
eventListener({
  payload: {
    uuid: 'request-1',
    actionType: 'sql_execution_event',
    message: { eventType: 'finished' },
  },
});
assert.deepEqual(JSON.parse(pushedMessage), {
  uuid: 'request-1',
  actionType: 'sql_execution_event',
  message: { eventType: 'finished' },
});

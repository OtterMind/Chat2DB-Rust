import assert from 'node:assert/strict';
import {
  DEFAULT_SQL_COMPLETION_ACCEPT_KEY,
  getSqlCompletionAcceptKey,
  getSqlCompletionAcceptKeyOptions,
} from './sqlCompletionAcceptKey';

assert.equal(DEFAULT_SQL_COMPLETION_ACCEPT_KEY, 'enter', 'SQL completion accepts by Enter by default');
assert.equal(getSqlCompletionAcceptKey(undefined), 'enter', 'missing accept key falls back to Enter');
assert.equal(getSqlCompletionAcceptKey('enter'), 'enter', 'explicit Enter accept key is preserved');
assert.equal(getSqlCompletionAcceptKey('tab'), 'tab', 'explicit Tab accept key is preserved');

assert.deepEqual(
  getSqlCompletionAcceptKeyOptions('enter'),
  {
    acceptSuggestionOnEnter: 'on',
    tabCompletion: 'off',
  },
  'Enter mode accepts suggestions on Enter and disables Tab completion',
);

assert.deepEqual(
  getSqlCompletionAcceptKeyOptions('tab'),
  {
    acceptSuggestionOnEnter: 'off',
    tabCompletion: 'on',
  },
  'Tab mode accepts suggestions on Tab and disables Enter acceptance',
);

console.log('sqlCompletionAcceptKey tests passed');


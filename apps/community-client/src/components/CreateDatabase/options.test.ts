import assert from 'node:assert/strict';
import { buildCharsetOptions, buildCollationOptions } from './options';

const charsets = [
  { charsetName: 'binary', defaultCollationName: 'binary' },
  { charsetName: 'utf8mb4', defaultCollationName: 'utf8mb4_0900_ai_ci' },
];
const collations = [
  { collationName: 'binary' },
  { collationName: 'latin1_swedish_ci' },
  { collationName: 'utf8mb4_0900_ai_ci' },
  { collationName: 'utf8mb4_general_ci' },
];

assert.deepEqual(buildCharsetOptions(charsets), [
  { label: 'binary', value: 'binary' },
  { label: 'utf8mb4', value: 'utf8mb4' },
]);
assert.deepEqual(buildCollationOptions(charsets, collations, 'binary'), [{ label: 'binary', value: 'binary' }]);
assert.deepEqual(buildCollationOptions(charsets, collations, 'UTF8MB4'), [
  { label: 'utf8mb4_0900_ai_ci', value: 'utf8mb4_0900_ai_ci' },
  { label: 'utf8mb4_general_ci', value: 'utf8mb4_general_ci' },
]);
assert.equal(buildCollationOptions(charsets, collations).length, collations.length);

console.log('CreateDatabase options tests passed');

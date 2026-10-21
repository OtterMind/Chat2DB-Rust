import { resolveResultSetEditor } from './editorType';

function assertEqual(actual: any, expected: any, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

assertEqual(resolveResultSetEditor('DATE'), 'custom-date-editor', 'DATE maps to date editor');
assertEqual(resolveResultSetEditor('TIME'), 'custom-time-editor', 'TIME maps to time editor');
assertEqual(resolveResultSetEditor('DATETIME'), 'custom-datetime-editor', 'DATETIME maps to datetime editor');
assertEqual(resolveResultSetEditor('TIMESTAMP'), 'custom-timestamp-editor', 'TIMESTAMP maps to timestamp editor');
assertEqual(resolveResultSetEditor('TEXT'), 'custom-input-editor', 'TEXT falls back to text editor');
assertEqual(resolveResultSetEditor(undefined), 'custom-input-editor', 'missing editor type falls back to text editor');
assertEqual(resolveResultSetEditor('UNSUPPORTED'), 'custom-input-editor', 'unknown editor type falls back to text editor');

console.log('ResultSetTable editorType tests passed');

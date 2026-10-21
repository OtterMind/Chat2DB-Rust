import assert from 'node:assert/strict';
import {
  getBackendCompletionItemEffectiveFilterText,
  getBackendCompletionItemFilterText,
  getBackendCompletionItemDescription,
  getBackendCompletionItemDetail,
  getBackendCompletionItemInsertText,
  getBackendCompletionItemLabel,
  shouldTriggerQualifiedReferenceCompletion,
} from './sqlCompletionBackendSuggestion';

const columnLabel = getBackendCompletionItemLabel({
  type: 'COLUMN',
  label: 'id',
  detail: 'BIGINT',
  dataType: 'BIGINT',
  tableAlias: 'a',
  tableName: 'access_control_apply_record',
});

assert.deepEqual(
  columnLabel,
  {
    label: 'id',
    detail: '(a)',
    description: 'BIGINT',
  },
  'column label keeps relation detail separate from data type description',
);

assert.equal(
  getBackendCompletionItemDetail({
    type: 'COLUMN',
    label: 'id',
    detail: 'BIGINT',
    dataType: 'BIGINT',
    tableName: 'access_control_apply_record',
  }),
  '(access_control_apply_record)',
  'column detail falls back to table name',
);

assert.equal(
  getBackendCompletionItemDescription({
    type: 'COLUMN',
    label: 'id',
    detail: 'BIGINT',
  }),
  'BIGINT',
  'column description tolerates legacy backend detail-as-type payloads',
);

assert.deepEqual(
  getBackendCompletionItemLabel({
    type: 'TABLE',
    label: 'access_control_apply_record',
    detail: '(enterprise_gateway_dev)',
    description: '@localhost',
  }),
  {
    label: 'access_control_apply_record',
    detail: '(enterprise_gateway_dev)',
    description: '@localhost',
  },
  'non-column labels preserve backend detail and description',
);

assert.deepEqual(
  getBackendCompletionItemLabel({
    type: 'VARIABLE',
    label: 'a',
    detail: 'INT',
    description: 'Variable',
    dataType: 'INT',
  }),
  {
    label: 'a',
    description: 'INT',
  },
  'variable label shows the data type as description, not inline detail',
);

assert.equal(
  getBackendCompletionItemDetail({
    type: 'VARIABLE',
    label: 'a',
    detail: 'INT',
    dataType: 'INT',
  }),
  '',
  'variable detail is hidden from the inline label detail area',
);

assert.equal(
  getBackendCompletionItemDescription({
    type: 'VARIABLE',
    label: 'a',
    detail: 'INT',
  }),
  '',
  'variable description only uses the new dataType field',
);

assert.equal(
  getBackendCompletionItemFilterText({
    type: 'ALIAS',
    label: 'asdasdasda',
    insertText: 'asdasdasda.',
  }),
  'asdasdasda',
  'alias candidates filter by visible label, not dot-qualified insert text',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'DATABASE',
    label: 'information_schema',
    insertText: 'information_schema.',
  }),
  true,
  'database candidates that insert a trailing dot trigger the next scoped completion',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'SCHEMA',
    label: 'public',
    insertText: 'public.',
  }),
  true,
  'schema candidates that insert a trailing dot trigger the next scoped completion',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'TABLE',
    label: 'orders',
    insertText: 'orders.',
  }),
  true,
  'table candidates that insert a trailing dot trigger the next scoped completion',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'ALIAS',
    label: 'o',
    insertText: 'o.',
  }),
  true,
  'alias candidates keep triggering member completion after insertion',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'COLUMN',
    label: 'id',
    insertText: 'id.',
  }),
  false,
  'column candidates never trigger qualified reference completion',
);

assert.equal(
  shouldTriggerQualifiedReferenceCompletion({
    type: 'DATABASE',
    label: 'enterprise_gateway_dev',
    insertText: 'enterprise_gateway_dev',
  }),
  false,
  'plain database insertion without a trailing dot does not retrigger completion',
);

const userVariableCandidate = {
  type: 'VARIABLE',
  label: '@dasdsa',
  insertText: '@dasdsa',
};

assert.equal(
  getBackendCompletionItemInsertText(userVariableCandidate, true),
  'dasdsa',
  'user variable insert text drops @ when the editor already has the @ marker',
);

assert.equal(
  getBackendCompletionItemEffectiveFilterText(userVariableCandidate, 'dasdsa'),
  'dasdsa',
  'user variable filter text follows suffix insert text so Monaco can match the typed prefix',
);

assert.equal(
  getBackendCompletionItemInsertText(userVariableCandidate, false),
  '@dasdsa',
  'user variable insert text keeps @ when completion starts before the marker',
);

console.log('sqlCompletionBackendSuggestion tests passed');

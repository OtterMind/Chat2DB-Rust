import assert from 'node:assert/strict';
import { shouldTriggerSnippetPlaceholderCompletion } from './sqlCompletionSnippetNavigation';

assert.equal(
  shouldTriggerSnippetPlaceholderCompletion(
    {
      modelVersionId: 10,
      cursor: 20,
      selectionStart: 20,
      selectionEnd: 25,
    },
    {
      modelVersionId: 10,
      cursor: 40,
      selectionStart: 40,
      selectionEnd: 45,
    },
  ),
  true,
  'snippet placeholder navigation triggers when the cursor or selection moves without a content change',
);

assert.equal(
  shouldTriggerSnippetPlaceholderCompletion(
    {
      modelVersionId: 10,
      cursor: 20,
      selectionStart: 20,
      selectionEnd: 25,
    },
    {
      modelVersionId: 11,
      cursor: 25,
      selectionStart: 25,
      selectionEnd: 25,
    },
  ),
  false,
  'completion accept or indentation does not trigger because it changes the model version',
);

assert.equal(
  shouldTriggerSnippetPlaceholderCompletion(
    {
      modelVersionId: 10,
      cursor: 20,
      selectionStart: 20,
      selectionEnd: 25,
    },
    {
      modelVersionId: 10,
      cursor: 20,
      selectionStart: 20,
      selectionEnd: 25,
    },
  ),
  false,
  'Tab without a placeholder movement does not retrigger completion',
);

assert.equal(
  shouldTriggerSnippetPlaceholderCompletion(null, {
    modelVersionId: 10,
    cursor: 20,
    selectionStart: 20,
    selectionEnd: 25,
  }),
  false,
  'missing snapshots are ignored',
);

console.log('sqlCompletionSnippetNavigation tests passed');

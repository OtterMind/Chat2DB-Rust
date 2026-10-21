import assert from 'node:assert/strict';
import {
  getCompletionLabelParts,
  getBoundedEditorSuggestMaxWidth,
  getSuggestWidgetHorizontalOffset,
  measureCompletionSuggestWidth,
} from './sqlCompletionSuggestWidth';

assert.deepEqual(
  getCompletionLabelParts({
    label: {
      label: 'id',
      detail: '(a)',
      description: 'BIGINT',
    },
  } as any),
  {
    label: 'id',
    detail: '(a)',
    description: 'BIGINT',
  },
  'object labels expose label, detail, and description',
);

assert.deepEqual(
  getCompletionLabelParts({
    label: 'SELECT',
  } as any),
  {
    label: 'SELECT',
    detail: '',
    description: '',
  },
  'string labels are supported',
);

assert.equal(
  measureCompletionSuggestWidth(
    [
      {
        label: {
          label: 'id',
          detail: '(a)',
          description: 'BIGINT',
        },
      } as any,
    ],
    {
      minWidth: 100,
      maxWidth: 500,
      extraWidth: 20,
      partGap: 10,
      measureText: (text) => text.length * 10,
    },
  ),
  150,
  'width includes label, detail, description, gaps, and extra width',
);

assert.equal(
  measureCompletionSuggestWidth(
    [{ label: 'id' } as any],
    {
      minWidth: 360,
      maxWidth: 500,
      extraWidth: 20,
      measureText: (text) => text.length * 10,
    },
  ),
  360,
  'width respects the minimum width',
);

assert.equal(
  measureCompletionSuggestWidth(
    [
      {
        label: {
          label: 'access_control_apply_record',
          detail: '(enterprise_gateway_dev)',
          description: '@localhost',
        },
      } as any,
    ],
    {
      minWidth: 100,
      maxWidth: 240,
      extraWidth: 20,
      partGap: 10,
      measureText: (text) => text.length * 10,
    },
  ),
  240,
  'width respects the maximum width',
);

assert.equal(
  getSuggestWidgetHorizontalOffset({ left: 120, right: 520, width: 400 }, 100, 600),
  0,
  'horizontal offset is zero when widget already fits',
);

assert.equal(
  getSuggestWidgetHorizontalOffset({ left: 420, right: 820, width: 400 }, 100, 600),
  -220,
  'horizontal offset pulls overflowing widget back from the right edge',
);

assert.equal(
  getSuggestWidgetHorizontalOffset({ left: 60, right: 460, width: 400 }, 100, 600),
  40,
  'horizontal offset pulls overflowing widget back from the left edge',
);

assert.equal(
  getSuggestWidgetHorizontalOffset({ left: 420, right: 1120, width: 700 }, 100, 600),
  -320,
  'horizontal offset aligns over-wide widgets to the left boundary',
);

assert.equal(
  getBoundedEditorSuggestMaxWidth(2000),
  960,
  'editor suggest max width is capped by the absolute max width',
);

assert.equal(
  getBoundedEditorSuggestMaxWidth(1000),
  708,
  'editor suggest max width uses a bounded ratio of editor width',
);

assert.equal(
  getBoundedEditorSuggestMaxWidth(320),
  304,
  'editor suggest max width still fits small editors',
);

console.log('sqlCompletionSuggestWidth tests passed');

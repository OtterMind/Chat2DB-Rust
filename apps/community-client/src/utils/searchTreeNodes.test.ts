import assert from 'node:assert/strict';
import type { TreeNodeData } from '@/typings';
import { TreeNodeType } from '../constants/tree';
import { filterTreeNodesForDisplay } from './filterTreeNodes';
import { searchTreeNodes } from './searchTreeNodes';

const node = (key: string, originalTitle: string, children?: TreeNodeData[], describe?: string): TreeNodeData => ({
  key,
  originalTitle,
  title: null,
  treeNodeType: 'TABLE' as TreeNodeData['treeNodeType'],
  extraParams: {},
  children,
  describe,
});

const tree = [
  node('schema-app', 'app', [
    node('table-orders', 'orders', [node('column-orders-id', 'orders_id')]),
    node('table-attendance', 'sms_attendance', undefined, 'student attendance records'),
  ]),
];

const parentAndChildMatches = searchTreeNodes(tree, 'orders');
assert.deepEqual(parentAndChildMatches.matchedKeys, ['table-orders', 'column-orders-id']);
assert.equal(parentAndChildMatches.matchedNodes.length, 1);

const childOnlyMatch = searchTreeNodes(tree, 'attendance');
assert.deepEqual(childOnlyMatch.matchedKeys, ['table-attendance']);
assert.equal(childOnlyMatch.matchedNodes[0].key, 'schema-app');
assert.deepEqual(childOnlyMatch.parentIdsWithMatches, ['schema-app']);

const noMatches = searchTreeNodes(tree, 'missing');
assert.deepEqual(noMatches.matchedKeys, []);
assert.deepEqual(noMatches.matchedNodes, []);

const visibleTree = filterTreeNodesForDisplay(
  [
    {
      ...node('data-source', 'local'),
      treeNodeType: TreeNodeType.DATA_SOURCE,
      extraParams: { dataSourceId: 1 },
      children: [
        {
          ...node('schema-hidden', 'private_schema', [node('table-hidden', 'hidden_orders')]),
          treeNodeType: TreeNodeType.SCHEMA,
        },
        {
          ...node('schema-visible', 'public_schema', [node('table-visible', 'visible_orders')]),
          treeNodeType: TreeNodeType.SCHEMA,
        },
      ],
    },
  ],
  { hiddenTreeNodeIds: { 1: ['schema-hidden'] } },
);
const visibleMatches = searchTreeNodes(visibleTree, 'orders');
assert.deepEqual(visibleMatches.matchedKeys, ['table-visible']);

console.log('searchTreeNodes.test.ts: all assertions passed');

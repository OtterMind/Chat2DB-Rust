import assert from 'node:assert/strict';
import { TreeNodeType } from '@/constants/tree';
import type { TreeNodeData } from '@/typings';
import { applyDatabaseObjectTreeSorting } from './sortTreeNodes';

function treeNode(
  originalTitle: string,
  treeNodeType: TreeNodeType,
  options?: { pinned?: boolean; isLeaf?: boolean; children?: TreeNodeData[] },
): TreeNodeData {
  return {
    key: originalTitle,
    originalTitle,
    treeNodeType,
    isLeaf: options?.isLeaf ?? false,
    extraParams: {},
    decorativeParams: { pinned: options?.pinned },
    children: options?.children,
  };
}

function freezeTree(nodes: TreeNodeData[]) {
  nodes.forEach((node) => {
    if (node.children) {
      freezeTree(node.children);
    }
    Object.freeze(node.decorativeParams);
    Object.freeze(node);
  });
  Object.freeze(nodes);
}

const originalNodes = [
  treeNode('Zulu source', TreeNodeType.DATA_SOURCE, {
    children: [
      treeNode('Views', TreeNodeType.VIEWS, {
        children: [
          treeNode('view10', TreeNodeType.VIEW),
          treeNode('view2', TreeNodeType.VIEW),
        ],
      }),
      treeNode('Tables', TreeNodeType.TABLES, {
        children: [
          treeNode('table10', TreeNodeType.TABLE, {
            children: [
              treeNode('column10', TreeNodeType.COLUMN, { isLeaf: true }),
              treeNode('beta', TreeNodeType.COLUMN, { isLeaf: true }),
              treeNode('column2', TreeNodeType.COLUMN, { isLeaf: true }),
              treeNode('Zulu', TreeNodeType.COLUMN, { isLeaf: true, pinned: true }),
              treeNode('alpha', TreeNodeType.COLUMN, { isLeaf: true, pinned: true }),
            ],
          }),
          treeNode('beta', TreeNodeType.TABLE),
          treeNode('table2', TreeNodeType.TABLE),
        ],
      }),
    ],
  }),
  treeNode('Alpha source', TreeNodeType.DATA_SOURCE),
];

freezeTree(originalNodes);
const unchangedNodes = applyDatabaseObjectTreeSorting(originalNodes, false);
assert.strictEqual(unchangedNodes, originalNodes);

const sortedNodes = applyDatabaseObjectTreeSorting(originalNodes, true);
assert.deepEqual(
  sortedNodes.map((node) => node.originalTitle),
  ['Zulu source', 'Alpha source'],
  'data source order must remain unchanged',
);
assert.deepEqual(
  sortedNodes[0].children?.map((node) => node.originalTitle),
  ['Views', 'Tables'],
  'directory order must remain unchanged',
);
assert.deepEqual(
  sortedNodes[0].children?.[0].children?.map((node) => node.originalTitle),
  ['view10', 'view2'],
  'lazy expandable nodes without children are not leaves',
);
assert.deepEqual(
  sortedNodes[0].children?.[1].children?.map((node) => node.originalTitle),
  ['table10', 'beta', 'table2'],
  'expandable table nodes must retain backend order',
);
assert.deepEqual(
  sortedNodes[0].children?.[1].children?.[0].children?.map((node) => node.originalTitle),
  ['alpha', 'Zulu', 'beta', 'column2', 'column10'],
  'leaf nodes use pinned-first natural name order',
);
assert.deepEqual(
  originalNodes[0].children?.[1].children?.[0].children?.map((node) => node.originalTitle),
  ['column10', 'beta', 'column2', 'Zulu', 'alpha'],
  'display sorting must not mutate the store order',
);

const mixedNodes = [
  treeNode('leaf10', TreeNodeType.COLUMN, { isLeaf: true }),
  treeNode('leaf2', TreeNodeType.KEY, { isLeaf: true }),
  treeNode('Directory', TreeNodeType.COLUMNS, {
    children: [
      treeNode('inner10', TreeNodeType.INDEX, { isLeaf: true }),
      treeNode('inner2', TreeNodeType.INDEX, { isLeaf: true }),
    ],
  }),
  treeNode('Zulu', TreeNodeType.ALL_DATA, { isLeaf: true, pinned: true }),
  treeNode('alpha', TreeNodeType.ALL_DATA, { isLeaf: true, pinned: true }),
  treeNode('tailB', TreeNodeType.ALL_DATA, { isLeaf: true }),
  treeNode('tailA', TreeNodeType.ALL_DATA, { isLeaf: true }),
];
const sortedMixedNodes = applyDatabaseObjectTreeSorting(mixedNodes, true);
assert.deepEqual(
  sortedMixedNodes.map((node) => node.originalTitle),
  ['leaf2', 'leaf10', 'Directory', 'alpha', 'Zulu', 'tailA', 'tailB'],
  'only consecutive leaf segments are sorted and new leaf types need no allowlist entry',
);
assert.deepEqual(
  sortedMixedNodes[2].children?.map((node) => node.originalTitle),
  ['inner2', 'inner10'],
  'leaf sorting applies recursively below any parent',
);

const groupedSources = [
  treeNode('Group', TreeNodeType.GROUP, {
    children: [
      treeNode('Second source', TreeNodeType.DATA_SOURCE),
      treeNode('First source', TreeNodeType.DATA_SOURCE),
    ],
  }),
];
const sortedGroupedSources = applyDatabaseObjectTreeSorting(groupedSources, true);
assert.deepEqual(
  sortedGroupedSources[0].children?.map((node) => node.originalTitle),
  ['Second source', 'First source'],
  'data source order inside a group must remain unchanged',
);

console.log('NewTree optional leaf-node ordering tests passed');

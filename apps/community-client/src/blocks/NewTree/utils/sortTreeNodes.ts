import type { TreeNodeData } from '@/typings';

const nameCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
});

export function compareNames(left: string, right: string) {
  return nameCollator.compare(left, right);
}

function compareLeafNodes(left: TreeNodeData, right: TreeNodeData) {
  const pinnedOrder =
    Number(Boolean(right.decorativeParams?.pinned)) - Number(Boolean(left.decorativeParams?.pinned));
  return pinnedOrder || compareNames(left.originalTitle, right.originalTitle);
}

function sortLeafSegments(nodes: TreeNodeData[]) {
  const result = [...nodes];
  let segmentStart = 0;

  while (segmentStart < result.length) {
    if (result[segmentStart].isLeaf !== true) {
      segmentStart += 1;
      continue;
    }

    let segmentEnd = segmentStart + 1;
    while (segmentEnd < result.length && result[segmentEnd].isLeaf === true) {
      segmentEnd += 1;
    }
    const sortedSegment = result.slice(segmentStart, segmentEnd).sort(compareLeafNodes);
    sortedSegment.forEach((node, index) => {
      result[segmentStart + index] = node;
    });
    segmentStart = segmentEnd;
  }

  return result;
}

function sortDatabaseObjectNodes(nodes: TreeNodeData[]): TreeNodeData[] {
  const nodesWithSortedChildren = nodes.map((node) => {
    if (!node.children?.length) {
      return node;
    }
    return {
      ...node,
      children: sortDatabaseObjectNodes(node.children),
    };
  });

  return sortLeafSegments(nodesWithSortedChildren);
}

export function applyDatabaseObjectTreeSorting(nodes: TreeNodeData[], enabled: boolean) {
  return enabled ? sortDatabaseObjectNodes(nodes) : nodes;
}

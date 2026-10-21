import type { TreeNodeData } from '@/typings';

export function searchTreeNodes(
  treeNodes: TreeNodeData[],
  searchValue: string,
): { matchedNodes: TreeNodeData[]; matchedKeys: string[]; parentIdsWithMatches: string[] } {
  const result: TreeNodeData[] = [];
  const matchedKeys: string[] = [];
  const parentIdsWithMatches: string[] = [];

  function collectMatchedKeys(nodes: TreeNodeData[], value: string) {
    nodes.forEach((node) => {
      if (isMatched(value, [node.originalTitle, node.describe || ''])) {
        matchedKeys.push(String(node.key));
      }
      if (node.children) {
        collectMatchedKeys(node.children, value);
      }
    });
  }

  function traverse(node: TreeNodeData, value: string): TreeNodeData | null {
    let hasMatchingChild = false;

    if (isMatched(value, [node.originalTitle, node.describe || ''])) {
      return { ...node };
    }
    if (node.children) {
      const matchingChildren = node.children
        .map((child) => {
          const match = traverse(child, value);
          if (match) hasMatchingChild = true;
          return match;
        })
        .filter((match): match is TreeNodeData => Boolean(match));

      if (matchingChildren.length > 0) {
        if (hasMatchingChild) {
          parentIdsWithMatches.push(node.key as string);
        }
        return { ...node, children: matchingChildren };
      }
    }
    return null;
  }

  treeNodes.forEach((node) => {
    const matchingNode = traverse(node, searchValue);
    if (matchingNode) {
      result.push(matchingNode);
    }
  });

  collectMatchedKeys(treeNodes, searchValue);

  return { matchedNodes: result, matchedKeys, parentIdsWithMatches };
}

export function isMatched(searchValue: string, originalTitle: string | string[]) {
  const modifiedSearchValue = searchValue.replace(/_/g, '');
  if (Array.isArray(originalTitle)) {
    return originalTitle.some((title) => new RegExp(modifiedSearchValue, 'gi').test(title.replace(/_/g, '')));
  }
  return new RegExp(modifiedSearchValue, 'gi').test(originalTitle.replace(/_/g, ''));
}

export function isMatchedAndReplace(searchValue: string, originalTitle: string) {
  return originalTitle?.replace(
    new RegExp(searchValue, 'gi'),
    (matched) => `<span style='color:red;'>${matched}</span>`,
  );
}

import { flatMap, omit } from 'lodash';

// tree flattening
export function flattenTree<T extends { [key: string]: any }>(
  tree: T[],
  childKey: keyof T = 'children',
): Omit<T, typeof childKey>[] {
  return flatMap(tree, (node) => {
    const children = (node[childKey] || []) as T[];
    return [{ ...omit(node, childKey) }, ...flattenTree(children, childKey)];
  });
}

// //Perform flattening
// const flatTree = flattenTree(tree);
// console.log(flatTree);

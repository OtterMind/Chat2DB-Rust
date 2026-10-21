import { loadNamespaceTree } from './loadNamespaceTree';

async function testRejectedRequestSettlesWithEmptyTree() {
  let treeData: string[] | null = null;

  await loadNamespaceTree(
    () => Promise.reject(new Error('request failed')),
    (items) => {
      treeData = items;
    },
  );

  if (treeData === null || treeData.length !== 0) {
    throw new Error(`expected an empty settled tree, got ${JSON.stringify(treeData)}`);
  }
}

async function testFulfilledRequestPreservesTreeItems() {
  let treeData: string[] | null = null;

  await loadNamespaceTree(
    () => Promise.resolve(['datasource']),
    (items) => {
      treeData = items;
    },
  );

  if (JSON.stringify(treeData) !== JSON.stringify(['datasource'])) {
    throw new Error(`expected the loaded tree items, got ${JSON.stringify(treeData)}`);
  }
}

Promise.all([testRejectedRequestSettlesWithEmptyTree(), testFulfilledRequestPreservesTreeItems()])
  .then(() => {
    console.log('Tree namespace loading tests passed');
  })
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });

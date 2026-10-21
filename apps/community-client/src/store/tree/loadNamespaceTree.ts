export async function loadNamespaceTree<T>(request: () => Promise<T[]>, settle: (items: T[]) => void): Promise<void> {
  let items: T[];
  try {
    items = await request();
  } catch {
    items = [];
  }
  settle(items);
}

import type { RedisDataItem } from '@/typings/redis';
import { redisKeyRowIdentity } from './redisRowIdentity';

const REDIS_KEY_SEPARATOR = ':';
const nameCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
});

interface MutableRedisKeyGroup {
  key: string;
  title: string;
  count: number;
  groups: Map<string, MutableRedisKeyGroup>;
  leaves: RedisKeyTreeNode[];
}

export interface RedisKeyTreeNode {
  key: string;
  name: string;
  kind: 'group' | 'key';
  count?: number;
  rowIndex?: number;
  type?: RedisDataItem['type'];
  ttl?: RedisDataItem['ttl'];
  children?: RedisKeyTreeNode[];
}

function groupNodeKey(path: string[]) {
  return `redis-group:${path.map(encodeURIComponent).join('/')}`;
}

export function getRedisTreeRowIndex(node: RedisKeyTreeNode) {
  return node.kind === 'key' ? node.rowIndex : undefined;
}

function createMutableGroup(key: string, title: string): MutableRedisKeyGroup {
  return {
    key,
    title,
    count: 0,
    groups: new Map(),
    leaves: [],
  };
}

function compareTreeNodes(left: RedisKeyTreeNode, right: RedisKeyTreeNode) {
  if (left.kind !== right.kind) {
    return left.kind === 'group' ? -1 : 1;
  }
  return nameCollator.compare(left.name, right.name);
}

function finalizeGroup(group: MutableRedisKeyGroup): RedisKeyTreeNode {
  const children = [
    ...Array.from(group.groups.values()).map(finalizeGroup),
    ...group.leaves,
  ].sort(compareTreeNodes);

  return {
    key: group.key,
    name: group.title,
    kind: 'group',
    count: group.count,
    children,
  };
}

export function buildRedisKeyTree(items: RedisDataItem[]) {
  const root = createMutableGroup('redis-root', '');

  items.forEach((item, rowIndex) => {
    if (item.name === null) {
      return;
    }

    const segments = item.name.split(REDIS_KEY_SEPARATOR);
    const groupSegments = segments.slice(0, -1);
    const canGroup = groupSegments.length > 0 && groupSegments.every(Boolean);
    let parent = root;

    if (canGroup) {
      const path: string[] = [];
      groupSegments.forEach((segment) => {
        path.push(segment);
        let group = parent.groups.get(segment);
        if (!group) {
          group = createMutableGroup(groupNodeKey(path), segment);
          parent.groups.set(segment, group);
        }
        group.count += 1;
        parent = group;
      });
    }

    parent.leaves.push({
      key: redisKeyRowIdentity(item.name),
      name: item.name,
      kind: 'key',
      rowIndex,
      type: item.type,
      ttl: item.ttl,
    });
  });

  return [
    ...Array.from(root.groups.values()).map(finalizeGroup),
    ...root.leaves,
  ].sort(compareTreeNodes);
}

export function collectRedisGroupKeys(nodes: RedisKeyTreeNode[]) {
  const keys: string[] = [];

  const visit = (treeNodes: RedisKeyTreeNode[]) => {
    treeNodes.forEach((node) => {
      if (node.kind !== 'group') {
        return;
      }
      keys.push(node.key);
      if (node.children) {
        visit(node.children);
      }
    });
  };

  visit(nodes);
  return keys;
}

export function collectRedisBranchGroupKeys(nodes: RedisKeyTreeNode[]) {
  const keys: string[] = [];

  const visit = (treeNodes: RedisKeyTreeNode[]) => {
    treeNodes.forEach((node) => {
      if (node.kind !== 'group') {
        return;
      }
      const childGroups = node.children?.filter((child) => child.kind === 'group') || [];
      if (childGroups.length === 0) {
        return;
      }
      keys.push(node.key);
      visit(childGroups);
    });
  };

  visit(nodes);
  return keys;
}

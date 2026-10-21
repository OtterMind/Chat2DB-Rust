import assert from 'node:assert/strict';
import type { RedisDataItem } from '@/typings/redis';
import {
  buildRedisKeyTree,
  collectRedisBranchGroupKeys,
  collectRedisGroupKeys,
  getRedisTreeRowIndex,
} from './redisKeyTree';
import { redisKeyRowIdentity } from './redisRowIdentity';

const keys = ['user:10', 'plain', 'cache:user:2', 'user:2', 'user', 'broken::key'];
const tree = buildRedisKeyTree(keys.map((name) => ({ name } as RedisDataItem)));

assert.deepEqual(
  tree.map((node) => [node.kind, node.name]),
  [
    ['group', 'cache'],
    ['group', 'user'],
    ['key', 'broken::key'],
    ['key', 'plain'],
    ['key', 'user'],
  ],
);

const userGroup = tree[1];
assert.equal(userGroup.count, 2);
assert.deepEqual(
  userGroup.children?.map((node) => node.name),
  ['user:2', 'user:10'],
);
assert.equal(userGroup.children?.[0].rowIndex, 3);
assert.equal(getRedisTreeRowIndex(userGroup), undefined);
assert.equal(getRedisTreeRowIndex(userGroup.children![0]), 3);
assert.equal(userGroup.children?.[0].key, redisKeyRowIdentity('user:2'));
assert.deepEqual(collectRedisGroupKeys(tree), ['redis-group:cache', 'redis-group:cache/user', 'redis-group:user']);
assert.deepEqual(
  collectRedisBranchGroupKeys(tree),
  ['redis-group:cache'],
  'only directories whose direct children include another directory should expand by default',
);

console.log('Redis key hierarchy tests passed');

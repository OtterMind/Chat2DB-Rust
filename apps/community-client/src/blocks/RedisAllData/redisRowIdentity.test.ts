import assert from 'node:assert/strict';
import type { RedisDataItem } from '@/typings/redis';
import {
  getRedisDataItemIdentity,
  REDIS_DRAFT_ROW_IDENTITY,
  redisKeyRowIdentity,
  resolveRedisDataItem,
} from './redisRowIdentity';

const first = { name: 'user:1' } as RedisDataItem;
const second = { name: 'user:2' } as RedisDataItem;
const draft = { name: null, isDraftFE: true } as RedisDataItem;

assert.equal(getRedisDataItemIdentity(first), redisKeyRowIdentity('user:1'));
assert.equal(getRedisDataItemIdentity(draft), REDIS_DRAFT_ROW_IDENTITY);
assert.equal(getRedisDataItemIdentity({ name: null } as RedisDataItem), undefined);
assert.equal(redisKeyRowIdentity('bracket[key]:1'), 'redis-key:bracket%5Bkey%5D%3A1');

const selectedIdentity = redisKeyRowIdentity('user:2');
assert.deepEqual(resolveRedisDataItem([first, second], selectedIdentity), { item: second, index: 1 });
assert.deepEqual(resolveRedisDataItem([second, first], selectedIdentity), { item: second, index: 0 });
assert.equal(resolveRedisDataItem([first], selectedIdentity), undefined);

console.log('Redis row identity tests passed');

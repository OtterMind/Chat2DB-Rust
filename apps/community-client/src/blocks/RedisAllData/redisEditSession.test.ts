import assert from 'node:assert/strict';
import { RedisEditSessionRegistry } from './redisEditSession';

const registry = new RedisEditSessionRegistry();
const firstA = registry.begin('redis-key:a');
const firstB = registry.begin('redis-key:b');

assert.equal(registry.isLatest(firstA), true, 'a different identity must not supersede the first session');
assert.equal(registry.isLatest(firstB), true);

const secondA = registry.begin('redis-key:a');
assert.equal(registry.isLatest(firstA), false, 'a newer session permanently supersedes the same identity');
assert.equal(registry.isLatest(secondA), true);

// Closing the current UI session intentionally does not mutate the registry.
assert.equal(registry.isLatest(secondA), true);

registry.invalidateAll();
assert.equal(registry.isLatest(secondA), false, 'refresh/search reset invalidates all background saves');
assert.equal(registry.isLatest(firstB), false);

const thirdA = registry.begin('redis-key:a');
assert.equal(registry.isLatest(thirdA), true);
assert.equal(registry.isLatest(secondA), false);

console.log('Redis edit session tests passed');

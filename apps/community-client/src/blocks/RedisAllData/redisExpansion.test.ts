import assert from 'node:assert/strict';
import {
  INITIAL_REDIS_EXPANSION_STATE,
  redisExpansionReducer,
  type RedisExpansionState,
} from './redisExpansion';

function reconcile(
  state: RedisExpansionState,
  validGroupKeys: string[],
  automaticExpandedKeys: string[],
  searchKey = '',
) {
  return redisExpansionReducer(state, {
    type: 'reconcile',
    active: true,
    validGroupKeys,
    automaticExpandedKeys,
    searchKey,
  });
}

let state = reconcile(INITIAL_REDIS_EXPANSION_STATE, ['root', 'root/leaf'], ['root']);
assert.deepEqual(state.expandedKeys, ['root'], 'initial view expands branch directories only');

state = redisExpansionReducer(state, { type: 'userChange', expandedKeys: [] });
state = reconcile(state, [], []);
assert.deepEqual(state.expandedKeys, [], 'refresh prunes expanded keys while data is empty');
state = reconcile(state, ['root', 'root/leaf'], ['root']);
assert.deepEqual(state.expandedKeys, [], 'refresh restores a deliberate collapse after data returns');

state = reconcile(state, ['root', 'root/leaf', 'new', 'new/leaf'], ['root', 'new']);
assert.deepEqual(state.expandedKeys, ['new'], 'Load more expands a new branch without reopening root');

let reclassifiedState = reconcile(INITIAL_REDIS_EXPANSION_STATE, ['existing'], []);
reclassifiedState = redisExpansionReducer(reclassifiedState, {
  type: 'userChange',
  expandedKeys: ['existing'],
});
reclassifiedState = redisExpansionReducer(reclassifiedState, { type: 'userChange', expandedKeys: [] });
reclassifiedState = reconcile(reclassifiedState, ['existing', 'existing/child'], ['existing']);
assert.deepEqual(
  reclassifiedState.expandedKeys,
  [],
  'Load more does not reopen an explicitly collapsed directory that becomes a branch',
);

let searchState = reconcile(INITIAL_REDIS_EXPANSION_STATE, ['root', 'root/leaf'], ['root']);
searchState = redisExpansionReducer(searchState, { type: 'userChange', expandedKeys: [] });
searchState = reconcile(searchState, ['root', 'root/leaf'], ['root', 'root/leaf'], 'root:*');
assert.deepEqual(searchState.expandedKeys, ['root', 'root/leaf'], 'entering search reveals every ancestor');
searchState = redisExpansionReducer(searchState, { type: 'userChange', expandedKeys: ['root'] });
searchState = reconcile(searchState, ['root', 'root/leaf'], ['root', 'root/leaf'], 'root:*');
assert.deepEqual(searchState.expandedKeys, ['root'], 'detail updates do not reopen a search folder');
searchState = reconcile(searchState, [], [], 'root:*');
searchState = reconcile(searchState, ['root', 'root/leaf'], ['root', 'root/leaf'], 'root:*');
assert.deepEqual(searchState.expandedKeys, ['root'], 'search refresh preserves its temporary collapse');
searchState = reconcile(searchState, ['root', 'root/leaf'], ['root']);
assert.deepEqual(searchState.expandedKeys, [], 'clearing search restores the pre-search collapse');

let pruningState = reconcile(INITIAL_REDIS_EXPANSION_STATE, ['root', 'removed'], ['root']);
pruningState = redisExpansionReducer(pruningState, {
  type: 'userChange',
  expandedKeys: ['root', 'removed'],
});
pruningState = reconcile(pruningState, ['root'], ['root']);
assert.deepEqual(pruningState.expandedKeys, ['root'], 'reconcile prunes invalid expanded keys');

let resetState = reconcile(INITIAL_REDIS_EXPANSION_STATE, ['root'], ['root']);
resetState = redisExpansionReducer(resetState, { type: 'userChange', expandedKeys: [] });
resetState = redisExpansionReducer(resetState, {
  type: 'reconcile',
  active: false,
  validGroupKeys: [],
  automaticExpandedKeys: [],
  searchKey: '',
});
assert.deepEqual(resetState, INITIAL_REDIS_EXPANSION_STATE, 'leaving tree mode resets expansion intent');
resetState = reconcile(resetState, ['root'], ['root']);
assert.deepEqual(resetState.expandedKeys, ['root'], 'returning to tree mode reapplies the default policy');

console.log('Redis expansion tests passed');

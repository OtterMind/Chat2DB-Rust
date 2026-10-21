export interface RedisExpansionState {
  expandedKeys: string[];
  userExpandedKeys: string[];
  userCollapsedKeys: string[];
  searchCollapsedKeys: string[];
  searchKey: string;
  initialized: boolean;
}

export type RedisExpansionAction =
  | {
      type: 'reconcile';
      active: boolean;
      validGroupKeys: string[];
      automaticExpandedKeys: string[];
      searchKey: string;
    }
  | { type: 'userChange'; expandedKeys: string[] };

export const INITIAL_REDIS_EXPANSION_STATE: RedisExpansionState = {
  expandedKeys: [],
  userExpandedKeys: [],
  userCollapsedKeys: [],
  searchCollapsedKeys: [],
  searchKey: '',
  initialized: false,
};

function uniqueKeys(keys: string[]) {
  return Array.from(new Set(keys));
}

function equalKeys(left: string[], right: string[]) {
  return left.length === right.length && left.every((key, index) => key === right[index]);
}

function updateIntentKeys(current: string[], added: string[], removed: string[]) {
  const removedKeySet = new Set(removed);
  return uniqueKeys([...current.filter((key) => !removedKeySet.has(key)), ...added]);
}

function reconcileExpandedKeys(
  automaticExpandedKeys: string[],
  userExpandedKeys: string[],
  userCollapsedKeys: string[],
  validGroupKeySet: Set<string>,
) {
  const collapsedKeySet = new Set(userCollapsedKeys);
  return uniqueKeys([...automaticExpandedKeys, ...userExpandedKeys]).filter(
    (key) => validGroupKeySet.has(key) && !collapsedKeySet.has(key),
  );
}

export function redisExpansionReducer(
  state: RedisExpansionState,
  action: RedisExpansionAction,
): RedisExpansionState {
  if (action.type === 'userChange') {
    const expandedKeys = uniqueKeys(action.expandedKeys);
    if (equalKeys(state.expandedKeys, expandedKeys)) {
      return state;
    }

    const previousExpandedKeySet = new Set(state.expandedKeys);
    const expandedKeySet = new Set(expandedKeys);
    const openedKeys = expandedKeys.filter((key) => !previousExpandedKeySet.has(key));
    const collapsedKeys = state.expandedKeys.filter((key) => !expandedKeySet.has(key));

    if (state.searchKey) {
      return {
        ...state,
        expandedKeys,
        searchCollapsedKeys: updateIntentKeys(state.searchCollapsedKeys, collapsedKeys, openedKeys),
      };
    }

    return {
      ...state,
      expandedKeys,
      userExpandedKeys: updateIntentKeys(state.userExpandedKeys, openedKeys, collapsedKeys),
      userCollapsedKeys: updateIntentKeys(state.userCollapsedKeys, collapsedKeys, openedKeys),
    };
  }

  if (!action.active) {
    return state.initialized || state.expandedKeys.length > 0 ? INITIAL_REDIS_EXPANSION_STATE : state;
  }

  const validGroupKeySet = new Set(action.validGroupKeys);
  const automaticExpandedKeys = uniqueKeys(
    action.automaticExpandedKeys.filter((key) => validGroupKeySet.has(key)),
  );
  const searchChanged = state.searchKey !== action.searchKey;
  const searchCollapsedKeys = action.searchKey && !searchChanged ? state.searchCollapsedKeys : [];
  const expandedKeys = action.searchKey
    ? reconcileExpandedKeys(automaticExpandedKeys, [], searchCollapsedKeys, validGroupKeySet)
    : reconcileExpandedKeys(
        automaticExpandedKeys,
        state.userExpandedKeys,
        state.userCollapsedKeys,
        validGroupKeySet,
      );

  if (
    equalKeys(state.expandedKeys, expandedKeys) &&
    equalKeys(state.searchCollapsedKeys, searchCollapsedKeys) &&
    state.searchKey === action.searchKey &&
    state.initialized
  ) {
    return state;
  }
  return {
    ...state,
    expandedKeys,
    searchCollapsedKeys,
    searchKey: action.searchKey,
    initialized: true,
  };
}

import type { RedisDataItem } from '@/typings/redis';

export type RedisRowIdentity = string;

export const REDIS_DRAFT_ROW_IDENTITY: RedisRowIdentity = 'redis-draft:active';

export function redisKeyRowIdentity(redisKey: string): RedisRowIdentity {
  return `redis-key:${encodeURIComponent(redisKey)}`;
}

export function getRedisDataItemIdentity(item: RedisDataItem): RedisRowIdentity | undefined {
  if (item.name !== null) {
    return redisKeyRowIdentity(item.name);
  }
  return item.isDraftFE ? REDIS_DRAFT_ROW_IDENTITY : undefined;
}

export function resolveRedisDataItem(
  items: RedisDataItem[] | null | undefined,
  identity: RedisRowIdentity | null,
) {
  if (!items || !identity) {
    return undefined;
  }
  const index = items.findIndex((item) => getRedisDataItemIdentity(item) === identity);
  return index < 0 ? undefined : { item: items[index], index };
}

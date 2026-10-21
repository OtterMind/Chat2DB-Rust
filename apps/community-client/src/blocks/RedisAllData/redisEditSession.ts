import type { RedisRowIdentity } from './redisRowIdentity';

export interface RedisEditSessionToken {
  identity: RedisRowIdentity;
  generation: number;
  resetGeneration: number;
}

export class RedisEditSessionRegistry {
  private nextGeneration = 0;
  private resetGeneration = 0;
  private latestGenerationByIdentity = new Map<RedisRowIdentity, number>();

  begin(identity: RedisRowIdentity): RedisEditSessionToken {
    const generation = this.nextGeneration + 1;
    this.nextGeneration = generation;
    this.latestGenerationByIdentity.set(identity, generation);
    return {
      identity,
      generation,
      resetGeneration: this.resetGeneration,
    };
  }

  invalidateAll() {
    this.resetGeneration += 1;
    this.latestGenerationByIdentity.clear();
  }

  isLatest(session: RedisEditSessionToken | null | undefined) {
    return Boolean(
      session &&
        session.resetGeneration === this.resetGeneration &&
        this.latestGenerationByIdentity.get(session.identity) === session.generation,
    );
  }
}

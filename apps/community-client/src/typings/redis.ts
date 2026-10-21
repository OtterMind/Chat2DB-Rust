import { RedisFieldType, ActionType } from '@/constants/redis';

export interface FEItemMarker {
  id: string;
  action: ActionType;
}

export interface ListValue {
  action?: ActionType;
  value: string;
  index?: number;
}

export interface SetValues {
  action?: ActionType;
  value: string;
}

export interface HashValue {
  action?: ActionType;
  value: string;
  index?: number;
  field: string;
}

export interface ZSetValue {
  action?: ActionType;
  value: string;
  score: string;
  index?: number;
}

export interface StreamValue {
  action?: ActionType;
  id?: string;
  index?: number;
  values: {
    key: string;
    value: string | null;
  }[];
}

export interface RedisDataItem {
  name: string | null;
  type: RedisFieldType;
  ttl: number;
  value?: string;
  size?: number;
  listValues?: ListValue[];
  values?: SetValues[];
  zsValues?: ZSetValue[];
  hashValues?: HashValue[];
  streamValues?: StreamValue[];

  // Process some logic on the front end. Is it a draft?
  isDraftFE?: boolean;
}

export interface RedisKeyScanResult {
  keys: RedisDataItem[];
  nextCursor: string;
  hasMore: boolean;
  complete: boolean;
  stoppedReason: string;
  scanCalls: number;
  keysReturned: number;
  elapsedMs: number;
}

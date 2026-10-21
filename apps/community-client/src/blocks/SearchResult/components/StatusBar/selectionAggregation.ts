import type { SelectionMetricId } from '@/typings/settings';
import Big from 'big.js';

export interface SelectionAggregation {
  rowCount: number;
  count: number;
  nullCount: number;
  nonNullCount: number;
  uniqueCount: number;
  numericCount: number;
  sum: string;
  minimum: string | null;
  maximum: string | null;
  earliest: number | null;
  latest: number | null;
}

const STRICT_NUMBER = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?$/i;
const DATE_LIKE = /^\d{4}[-/]\d{1,2}[-/]\d{1,2}(?:[T\s].*)?$/;

const isNullValue = (value: unknown) => value === null || value === undefined;

const toDecimal = (value: unknown): Big | null => {
  let normalized: string;
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      return null;
    }
    normalized = String(value);
  } else {
    if (typeof value !== 'string') {
      return null;
    }
    normalized = value.trim();
    if (!normalized || !STRICT_NUMBER.test(normalized)) {
      return null;
    }
  }
  try {
    return new Big(normalized);
  } catch {
    return null;
  }
};

const toTimestamp = (value: unknown): number | null => {
  if (value instanceof Date) {
    const timestamp = value.getTime();
    return Number.isFinite(timestamp) ? timestamp : null;
  }
  if (typeof value !== 'string' || !DATE_LIKE.test(value.trim())) {
    return null;
  }
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : null;
};

const uniqueKey = (value: unknown): string => {
  if (typeof value === 'object') {
    try {
      return `object:${JSON.stringify(value)}`;
    } catch {
      return `object:${String(value)}`;
    }
  }
  return `${typeof value}:${String(value)}`;
};

export const summarizeSelection = (values: unknown[], rowCount: number): SelectionAggregation => {
  let nullCount = 0;
  let numericCount = 0;
  let sum = new Big(0);
  let numericMinimum: Big | null = null;
  let numericMaximum: Big | null = null;
  let stringMinimum: string | null = null;
  let stringMaximum: string | null = null;
  let earliest: number | null = null;
  let latest: number | null = null;
  const uniqueValues = new Set<string>();

  values.forEach((value) => {
    if (isNullValue(value)) {
      nullCount += 1;
      return;
    }

    uniqueValues.add(uniqueKey(value));
    const stringValue = String(value);
    stringMinimum =
      stringMinimum === null || stringValue.localeCompare(stringMinimum) < 0 ? stringValue : stringMinimum;
    stringMaximum =
      stringMaximum === null || stringValue.localeCompare(stringMaximum) > 0 ? stringValue : stringMaximum;

    const numericValue = toDecimal(value);
    if (numericValue !== null) {
      numericCount += 1;
      sum = sum.plus(numericValue);
      numericMinimum = numericMinimum === null || numericValue.lt(numericMinimum) ? numericValue : numericMinimum;
      numericMaximum = numericMaximum === null || numericValue.gt(numericMaximum) ? numericValue : numericMaximum;
    }

    const timestamp = toTimestamp(value);
    if (timestamp !== null) {
      earliest = earliest === null ? timestamp : Math.min(earliest, timestamp);
      latest = latest === null ? timestamp : Math.max(latest, timestamp);
    }
  });

  return {
    rowCount,
    count: values.length,
    nullCount,
    nonNullCount: values.length - nullCount,
    uniqueCount: uniqueValues.size,
    numericCount,
    sum: sum.toFixed(),
    minimum: numericCount ? numericMinimum?.toFixed() || null : stringMinimum,
    maximum: numericCount ? numericMaximum?.toFixed() || null : stringMaximum,
    earliest,
    latest,
  };
};

const formatRoundedNumber = (value: Big | string | number) => {
  return new Big(value).round(6)
    .toFixed();
};

const formatPercentage = (value: number, total: number) => {
  if (!total) {
    return '-';
  }
  return `${formatRoundedNumber(new Big(value).div(total)
    .times(100))}%`;
};

const formatDate = (timestamp: number | null) => {
  if (timestamp === null) {
    return '-';
  }
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(
    date.getMinutes(),
  )}:${pad(date.getSeconds())}`;
};

export const formatSelectionMetric = (metric: SelectionMetricId, summary: SelectionAggregation): string => {
  switch (metric) {
    case 'none':
      return '';
    case 'rowCount':
      return String(summary.rowCount);
    case 'count':
      return String(summary.count);
    case 'sum':
      return summary.numericCount ? summary.sum : '-';
    case 'average':
      return summary.numericCount ? formatRoundedNumber(new Big(summary.sum).div(summary.numericCount)) : '-';
    case 'minimum':
      return summary.minimum ?? '-';
    case 'maximum':
      return summary.maximum ?? '-';
    case 'nullCount':
      return String(summary.nullCount);
    case 'nonNullCount':
      return String(summary.nonNullCount);
    case 'uniqueCount':
      return String(summary.uniqueCount);
    case 'nullPercentage':
      return formatPercentage(summary.nullCount, summary.count);
    case 'nonNullPercentage':
      return formatPercentage(summary.nonNullCount, summary.count);
    case 'uniquePercentage':
      return formatPercentage(summary.uniqueCount, summary.count);
    case 'earliest':
      return formatDate(summary.earliest);
    case 'latest':
      return formatDate(summary.latest);
    default:
      return '-';
  }
};

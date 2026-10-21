import type i18n from '@/i18n';
import type { SelectionMetricId } from '@/typings/settings';

export interface SelectionMetricOption {
  id: SelectionMetricId;
  label: Parameters<typeof i18n>[0];
}

export const SELECTION_METRIC_OPTIONS: SelectionMetricOption[] = [
  { id: 'none', label: 'common.selectionAggregate.none' },
  { id: 'rowCount', label: 'common.selectionAggregate.rowCount' },
  { id: 'count', label: 'common.selectionAggregate.count' },
  { id: 'sum', label: 'common.selectionAggregate.sum' },
  { id: 'average', label: 'common.selectionAggregate.average' },
  { id: 'minimum', label: 'common.selectionAggregate.minimum' },
  { id: 'maximum', label: 'common.selectionAggregate.maximum' },
  { id: 'nullCount', label: 'common.selectionAggregate.nullCount' },
  { id: 'nonNullCount', label: 'common.selectionAggregate.nonNullCount' },
  { id: 'uniqueCount', label: 'common.selectionAggregate.uniqueCount' },
  { id: 'nullPercentage', label: 'common.selectionAggregate.nullPercentage' },
  { id: 'nonNullPercentage', label: 'common.selectionAggregate.nonNullPercentage' },
  { id: 'uniquePercentage', label: 'common.selectionAggregate.uniquePercentage' },
  { id: 'earliest', label: 'common.selectionAggregate.earliest' },
  { id: 'latest', label: 'common.selectionAggregate.latest' },
];

export const VISIBLE_SELECTION_METRIC_OPTIONS = SELECTION_METRIC_OPTIONS.filter((option) => option.id !== 'none');

import { memo, useMemo } from 'react';
import { Copy } from 'lucide-react';
import { staticMessage } from '@chat2db/ui';
import i18n from '@/i18n';
import { copyToClipboard } from '@/utils';
import { formatSelectionMetric, summarizeSelection } from '../StatusBar/selectionAggregation';
import { VISIBLE_SELECTION_METRIC_OPTIONS } from '../StatusBar/selectionMetrics';
import { useStyles } from './style';

interface IProps {
  selectedValues: unknown[];
  selectedRowCount: number;
}

const SelectionAggregates = ({ selectedValues, selectedRowCount }: IProps) => {
  const { styles } = useStyles();
  const summary = useMemo(
    () => summarizeSelection(selectedValues, selectedRowCount),
    [selectedValues, selectedRowCount],
  );

  if (!selectedValues.length) {
    return null;
  }

  return (
    <div className={styles.container}>
      {VISIBLE_SELECTION_METRIC_OPTIONS.map((option) => {
        const value = formatSelectionMetric(option.id, summary);
        const copyValue = () => {
          if (copyToClipboard(value)) {
            staticMessage.success(i18n('common.button.copySuccessfully'));
          }
        };
        return (
          <div className={styles.item} key={option.id}>
            <span className={styles.label}>{i18n(option.label)}</span>
            <button
              type="button"
              className={styles.valueButton}
              aria-label={`${i18n('common.button.copy')} ${i18n(option.label)}`}
              title={value}
              onClick={copyValue}
            >
              <span className={styles.value}>{value}</span>
              <Copy className={styles.copyIcon} size={13} strokeWidth={1.75} />
            </button>
          </div>
        );
      })}
    </div>
  );
};

export default memo(SelectionAggregates);

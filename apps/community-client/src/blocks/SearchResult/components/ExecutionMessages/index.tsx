import { memo, useEffect, useRef } from 'react';
import { Empty, Tag } from 'antd';
import i18n from '@/i18n';
import { useStyles } from './style';

export interface IExecutionMessageItem {
  level?: string;
  message: string;
  comment?: string;
  resultSetId?: number;
  executionIndex?: number;
}

interface IProps {
  data: IExecutionMessageItem[];
}

const LEVEL_COLOR_MAP: Record<string, string> = {
  ERROR: 'error',
  WARN: 'warning',
  WARNING: 'warning',
  INFO: 'processing',
};

export default memo<IProps>(({ data }) => {
  const { styles } = useStyles();
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    container.scrollTop = container.scrollHeight;
  }, [data.length]);

  if (!data.length) {
    return (
      <div className={styles.empty}>
        <Empty description={i18n('common.text.noData')} />
      </div>
    );
  }

  return (
    <div className={styles.container} ref={containerRef}>
      <div className={styles.list}>
        {data.map((item, index) => {
          const level = (item.level || 'INFO').toUpperCase();
          const tagColor = LEVEL_COLOR_MAP[level] || 'default';
          const metaParts = [`#${item.executionIndex || index + 1}`];
          if (item.comment) {
            metaParts.push(item.comment);
          }
          if (item.resultSetId) {
            metaParts.push(`ResultSet ${item.resultSetId}`);
          }

          return (
            <div className={styles.item} key={`${item.executionIndex || index}-${item.resultSetId || 0}-${index}`}>
              <div className={styles.meta}>
                <Tag color={tagColor}>{level}</Tag>
                <span>{metaParts.join(' · ')}</span>
              </div>
              <pre className={styles.message}>{item.message}</pre>
            </div>
          );
        })}
      </div>
    </div>
  );
});

import { memo, useMemo } from 'react';
import { useStyles } from './style';
import { IconfontSvg } from '@chat2db/ui';
import { Tooltip } from 'antd';
import isEqual from 'lodash/isEqual';

interface IProps {
  className?: string;
  iconCode?: string;
  title: string;
  tips?: {
    text: string;
    url?: string;
  };
  size?: 'sm' | 'md';
}

export default memo<IProps>(
  (props) => {
    const { className, title, iconCode, tips, size = 'md' } = props;
    const { styles, cx } = useStyles({ size });
    const iconSize = useMemo(() => {
      switch (size) {
        case 'sm':
          return 22;
        case 'md':
          return 30;
        default:
          return 30;
      }
    }, [size]);
    return (
      <div className={cx(className, styles.modalTitle)}>
        {iconCode && <IconfontSvg code={iconCode} size={iconSize} className={styles.prefixIcon} />}
        <span className={styles.title}>{title}</span>
        {tips && (
          <Tooltip title={tips.text}>
            <a className={styles.tipsIconContainer} href={tips.url} target="_blank" rel="noreferrer">
              <IconfontSvg className={styles.tipsIcon} size="md" code="icon-question-mark-circle" />
            </a>
          </Tooltip>
        )}
      </div>
    );
  },
  (prevProps, nextProps) => {
    return isEqual(prevProps, nextProps);
  },
);

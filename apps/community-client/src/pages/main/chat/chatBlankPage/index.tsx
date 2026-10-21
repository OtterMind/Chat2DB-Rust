import { memo, useMemo } from 'react';
import { useStyles } from './style';
import emptyLight from '@/assets/img/empty/chat-guide.png';
import emptyDark from '@/assets/img/empty/chat-guide-dark.png';

import i18n from '@/i18n';
import { useTheme } from 'antd-style';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  const { appearance } = useTheme();
  const isDarkMode = useMemo(() => appearance.includes('dark'), [appearance]);

  return (
    <div className={cx(styles.chatBlankPage, className)}>
      <img style={{ width: '200px' }} src={isDarkMode ? emptyDark : emptyLight} />
      <div className={styles.text}>{i18n('chat.page.welcome')}</div>
    </div>
  );
});

import { memo } from 'react';
import styles from './index.less';
import classnames from 'classnames';
import Logo from '@/components/Logo';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  return (
    <div className={classnames(styles.openScreenAnimation, className)}>
      <Logo size={80} className={styles.brandLogo} />
      <div className={styles.brandName}>
        <div className={styles.textImg}>Chat2DB</div>
      </div>
    </div>
  );
});

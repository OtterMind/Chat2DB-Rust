import { memo } from 'react';
import classnames from 'classnames';
import Iconfont from '@/components/Iconfont';
import { useStyles } from './style';

interface IProps {
  className?: string;
  text?: string;
}

export default memo<IProps>((props) => {
  const { className, text } = props;
  const { styles } = useStyles();

  return (
    <div className={classnames(styles.notPermission, className)}>
      <div className={styles.notPermissionIconBox}>
        <Iconfont className={styles.notPermissionIcon} code="&#xe658;" />
      </div>
      <div className={styles.notPermissionIconTips}>{text}</div>
    </div>
  );
});

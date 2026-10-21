import React, { memo } from 'react';
import { useStyles } from './style';
import { Divider } from 'antd';

interface IProps {
  className?: string;
  title: string | React.ReactNode;
  describe: string | React.ReactNode;
}

export default memo<IProps>((props) => {
  const { className, title, describe } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.subsection, className)}>
      <div className={styles.title}>{title}</div>
      <div className={styles.describe}>{describe}</div>
      <Divider className={styles.divider} />
    </div>
  );
});

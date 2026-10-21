import React, { memo } from 'react';
import { useStyles } from './style';

interface IProps {
  className?: string;
  footerLeft?: React.ReactNode;
  footerRight?: React.ReactNode;
  footerLeftStyle?: string;
  footerRightStyle?: string;
}

export default memo<IProps>((props) => {
  const { className, footerLeft, footerRight, footerLeftStyle, footerRightStyle } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(className, styles.footer)}>
      <div className={cx(styles.footerLeft, footerLeftStyle)}>{footerLeft}</div>
      <div className={cx(styles.footerRight, footerRightStyle)}>{footerRight}</div>
    </div>
  );
});

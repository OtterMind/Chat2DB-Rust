import React, { memo } from 'react';
import { useStyles } from './style';
import { Iconfont } from '@chat2db/ui';

interface IProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  text: string;
}

export default memo<IProps>((props) => {
  const { className, text, ...restProps } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.aiEntryButton, className)} {...restProps}>
      <Iconfont
        className={styles.icon}
        code="icon-gradient-sparkles"
        gradientColor="radial-gradient(204.52% 161.18% at 0% 0%, #FF4C33 0%, #FA8837 29.5%, #F218F4 72.5%, #AD00FF 100%)"
      />
      {text}
    </div>
  );
});

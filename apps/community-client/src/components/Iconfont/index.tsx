import React from 'react';
import { useStyles } from './style';
interface IProps extends React.HTMLAttributes<HTMLElement> {
  code: string;
  box?: boolean;
  boxSize?: number;
  size?: number;
  className?: string;
  classNameBox?: string;
  active?: boolean;
}

const Iconfont = (props: IProps) => {
  // console.log(active);
  const { box, boxSize = 32, size = 14, className, classNameBox, active, ...args } = props;
  const { styles, cx } = useStyles({ iconBoxSize: boxSize, iconSize: size });
  return box ? (
    <div {...args} className={cx(classNameBox, styles.iconBox, { [styles.activeIconBox]: active })}>
      <i className={cx(className, styles.iconfont)}>{props.code}</i>
    </div>
  ) : (
    <i className={cx(styles.iconfont, className)} {...args}>
      {props.code}
    </i>
  );
};

export default Iconfont;

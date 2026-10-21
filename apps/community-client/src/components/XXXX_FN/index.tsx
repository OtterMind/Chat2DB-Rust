import { memo } from 'react';
import { useStyles } from './style';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  return <div className={cx(styles.container, className)}>demo</div>;
});

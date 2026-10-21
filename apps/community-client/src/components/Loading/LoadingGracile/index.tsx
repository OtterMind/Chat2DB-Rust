import { memo } from 'react';
import { useStyles } from './style';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(className, styles.spinner)}>
      {Array.from({ length: 12 }).map((_, index) => (
        <div key={index} className={styles.spinnerBlade} />
      ))}
    </div>
  );
});

import { memo, forwardRef, useImperativeHandle, ForwardedRef } from 'react';
// import i18n from '@/i18n';
import { useStyles } from './style';

interface IProps {
  className?: string;
}

export interface XXXX_FNRef {
  getX: () => number;
}

const XXXX_FN = forwardRef((props: IProps, ref: ForwardedRef<XXXX_FNRef>) => {
  const { className } = props;
  const { styles, cx } = useStyles();

  useImperativeHandle(ref, () => ({
    getX: () => {
      return 1;
    },
  }));

  return <div className={cx(className, styles.container)}>1</div>;
});

export default memo(XXXX_FN);

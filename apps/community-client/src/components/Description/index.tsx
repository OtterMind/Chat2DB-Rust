import { DivProps } from '@/typings/common';
import { createStyles } from 'antd-style';
import { ReactNode, memo } from 'react';

export interface DescriptionProps extends DivProps {
  children: ReactNode;
}

export const useStyles = createStyles(({ css, token }) => {
  return {
    description: css`
      color: ${token.colorTextSecondary};
      font-size: 13px;
    `,
  };
});
const Description = memo<DescriptionProps>(({ className, children, ...rest }) => {
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.description, className)} {...rest}>
      {children}
    </div>
  );
});

export default Description;

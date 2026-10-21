import { DivProps } from '@/typings/common';
import { createStyles } from 'antd-style';
import { ReactNode, memo } from 'react';

export interface PromptExampleProps extends DivProps {
  children: ReactNode;
}

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      border-radius: 6px;
      background-color: ${token.colorPrimaryBg};
      color: ${token.colorPrimary};
      border: 1px solid ${token.colorPrimary};
      padding: 4px 8px;
      font-size: 13px;
    `,
  };
});
const PromptExample = memo<PromptExampleProps>(({ className, children, ...rest }) => {
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.container, className)} {...rest}>
      {children}
    </div>
  );
});

export default PromptExample;

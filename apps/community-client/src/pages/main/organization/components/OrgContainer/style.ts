import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    wrapper: css`
      height: 100%;
      box-sizing: border-box;
      padding: 32px 24px;
    `,
    empty: css`
      display: flex;
      justify-content: center;
      align-items: center;
    `,
  };
});

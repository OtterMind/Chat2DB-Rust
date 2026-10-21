import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    a: css``,
    chartCard: css`
      box-sizing: border-box;
      background: ${token.colorBgContainer};
      border-radius: 8px;
      overflow: hidden;
    `,
  };
});

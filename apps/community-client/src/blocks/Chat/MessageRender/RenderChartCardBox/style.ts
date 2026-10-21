import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    dashboardCard: css`
      background-color: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      /* min-width: 100%; */
      height: 350px;
    `,
  };
});

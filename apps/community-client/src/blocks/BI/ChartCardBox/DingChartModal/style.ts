import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    select: css`
      width: 100%;
    `,
    createDashboard: css`
      border-top: 1px solid ${token.colorBorder};
      margin-top: 4px;
      padding-top: 8px;
      display: flex;
      gap: 8px;
    `,
  };
});

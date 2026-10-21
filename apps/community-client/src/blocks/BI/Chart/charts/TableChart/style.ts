import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    chartTable: css`
      margin: 4px;
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 4px;
      overflow: hidden;
      height: calc(100% - 8px);
      width: calc(100% - 8px);
    `,
  };
});

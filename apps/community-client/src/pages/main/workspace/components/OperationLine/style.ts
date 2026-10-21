import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    operationLine: css`
      flex-shrink: 0;
      border-top: 1px solid ${token.colorBorderSecondary};
      height: 30px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0px 4px;
    `,
    operationLineLeft: css`
      display: flex;
      gap: 4px;
    `,
    searchBox: css`
      flex-shrink: 0;
      padding: 4px;
      border-top: 1px solid ${token.colorBorderSecondary};
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    canvasTable: css`
      height: 100%;
    `,
    headerTooltip: css`
      border-radius: 4px;
      background-color: ${token.colorBgBase};
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
      padding: 6px;
    `,
    headerTooltipFirst: css`
      display: flex;
      align-items: center;
    `,
    columnName: css`
      font-size: 15px;
      color: ${token.colorText};
      margin-right: 4px;
    `,
    columnType: css`
      font-size: 14px;
      color: ${token.colorText};
    `,
    columnComment: css`
      font-size: 14px;
      color: ${token.colorText};
    `,
  };
});

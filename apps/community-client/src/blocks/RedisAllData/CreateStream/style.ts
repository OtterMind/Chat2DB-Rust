import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    createList: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    baseTable: css`
      flex: 1;
      height: 0px;
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 4px;
    `,
    listItem: css`
      flex-shrink: 0;
      height: 30px;
      line-height: 30px;
    `,
    operationLine: css`
      margin-top: 10px;
      font-size: 12px;
      color: ${token.colorTextSecondary};
    `,
    disabledToolbarBtn: css`
      color: ${token.colorTextDisabled};
      cursor: not-allowed;
    `,
  };
});

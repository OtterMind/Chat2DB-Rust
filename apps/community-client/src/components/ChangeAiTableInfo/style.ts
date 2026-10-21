import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    changeAiTableInfo: css`
      display: flex;
      flex-direction: column;
      height: 100%;
    `,
    actionBar: css`
      height: 34px;
      flex-shrink: 0;
      border-bottom: 1px solid ${token.colorBorderLayout};
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 0px 6px;
    `,
    toolbarBtn: css`
      padding: 2px 4px;
    `,
    tableInfo: css`
      flex: 1;
      display: flex;
      flex-direction: column;
      height: 0px;
    `,
    tableName: css`
      flex-shrink: 0;
    `,
    columnInfo: css`
      flex: 1;
      overflow-y: auto;
      height: 0px;
      padding: 0px 10px;
      display: flex;
      flex-direction: column;
      gap: 10px;
    `,
    columnInfoTable: css`
      flex: 1;
      height: 0px;
    `,
    otherInfo: css`
      flex-shrink: 0;
      padding: 10px;
      height: 200px;
    `,
    form: css`
      display: flex;
      gap: 24px;
      padding: 12px 10px;
      box-sizing: border-box;
      .ant-form-item-label {
        label {
          color: ${token.colorText};
          font-size: 14px;
        }
      }
      .ant-form-item {
        flex: 1;
        margin-bottom: 0px;
      }
      .ant-form-item:last-child {
        flex: 2;
      }
    `,
    spinBox: css`
      display: flex;
      justify-content: center;
      align-items: center;
      height: 0;
      flex: 1;
    `,
    noStyles: css`
      text-align: center;
    `,
    notEditable: css`
      color: ${token.colorTextSecondary};
      cursor: not-allowed;
      &:hover {
        background-color: transparent;
      }
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    table: css`
      height: 100%;
      .ant-table-container {
        border-bottom: 1px solid ${token.colorBorderSecondary} !important;
      }
      .ant-table-row-selected > .ant-table-cell {
        background-color: none;
      }
      .ant-table-cell {
        padding: 0px !important;
        height: 31px !important;
        box-sizing: border-box;
      }
      .ant-table-thead .ant-table-cell {
        padding: 4px 6px !important;
      }
      .ant-table-tbody > tr:last-child td {
        border-bottom: 0px;
      }
      .selected-row {
        background-color: ${token.colorPrimaryBgHover} !important;
      }
      .ant-table-cell-row-hover {
        background-color: ${token.colorPrimaryBgHover} !important;
      }
    `,
    columnInfo: css`
      flex: 1;
      height: 0px;
      padding: 0px 10px;
      box-sizing: border-box;
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
      height: 180px;
      border-top: 1px solid ${token.colorBorderSecondary};
    `,
  };
});

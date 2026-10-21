import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    supportBaseTableBox: css`
      height: 100%;
      background-color: ${token.colorFillQuaternary};
      /* Top table border. */
      .ant-table-wrapper .ant-table.ant-table-bordered > .ant-table-container {
        border-top: 0px;
      }
      /* Border on the last header cell. */
      .ant-table-wrapper .ant-table.ant-table-bordered > .ant-table-container > .ant-table-header > table > thead > tr {
        > th:last-child {
          border-right: 0px;
        }
      }
      /* Border on the last body cell. */
      .ant-table-wrapper .ant-table-bordered .ant-table-tbody-virtual .ant-table-row {
        .ant-table-cell:last-child {
          border-inline-end: 0;
        }
      }
      .ant-table-wrapper .ant-table-tbody-virtual .ant-table-tbody-virtual-holder-inner {
        .ant-table-row:last-child .ant-table-cell {
          border-bottom: 0px;
        }
      }
      .ant-table-wrapper .ant-table-thead > tr > th,
      .ant-table-wrapper .ant-table-cell {
        padding: 0px;
      }
      .ant-table-cell {
        height: 32px;
      }
      .backgroundColorFillQ {
        background-color: ${token.colorFillQuaternary};
      }
      .isHighlightColumns {
        background-color: ${token.colorPrimaryBg};
      }
      .isHighlightRows {
        background-color: ${token.colorPrimaryBg};
      }
    `,
    plainText: css`
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      height: 31px;
      line-height: 31px;
      padding: 0px 4px;
      font-weight: 400;
    `,
    // tableHeader: css`
    //   padding: 0px 4px;
    // `,
    tableCell: css`
      height: 100%;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
    isSelectedTableCell: css`
      background-color: ${token.colorPrimaryBgHover};
    `,
    editTextTextClass: css`
      padding: 0px 4px;
      &:hover {
        border-radius: 0px;
      }
    `,
    editTextInputClass: css`
      border-radius: 0px !important;
    `,
    copyButton: css`
      display: inline-block;
      text-align: center;
      margin-left: 6px;
      transform: translateY(2px);
    `,
    spinBox: css`
      height: 100%;
      width: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    tooltipTitle: css`
      width: 100%;
      white-space: pre-wrap;
    `,
    
  };
});

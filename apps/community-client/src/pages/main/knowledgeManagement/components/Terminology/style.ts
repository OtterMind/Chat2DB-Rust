import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      gap: 10px;
    `,
    tips: css`
      display: flex;
      align-items: center;
      gap: 4px;
      border: 1px solid ${token.colorPrimaryBgHover};
      background-color: ${token.colorPrimaryBg};
      border-radius: 4px;
      padding: 4px 8px;
      color: ${token.colorPrimary};
    `,
    tipsContent: css`
      flex:1;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      width: 0px;
      box-sizing: border-box;
    `,
    table: css`
      flex: 1;
      thead {
        .ant-table-cell {
          padding: 8px !important;
        }
      }
      .ant-table-cell {
        padding: 2px !important;
      }
      
    `,
    formItem: css`
      margin-bottom: 0px !important;
      height: 100%;
    `,
    cellPreview: css`
      line-height: 22px;
      padding: 0px 6px;
      height: auto;
      overflow: hidden;
      /* text-overflow: ellipsis;
      white-space: nowrap;
      box-sizing: border-box; */
    `,
    dataSourceLabelListView: css`
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
      height: auto;
    `,
    input: css`
      height: 30px;
      box-sizing: border-box;
      padding: 0px 6px;
      border-radius: 0px;
    `,
    selectInput: css`
      min-height: 30px;
      box-sizing: border-box;
      padding: 0px 6px;
      border-radius: 0px;
    `,
    dataSourceLabel: css`
      display: flex;
      align-items: center;
      gap: 4px;
    `,
    dataSourceLabelView: css`
      display: flex;
      align-items: center;
      gap: 4px;
      width: fit-content;
      border-radius: 4px;
      padding: 2px 8px;
      background-color: ${token.colorPrimaryBg};
      height: 22px;
    `,
    header: css`
      display: flex;
      justify-content: space-between;
      flex-wrap: wrap;
      margin: 16px 0px 0px;
    `,
    searchInput: css`
      width: 200px;
      flex-shrink: 0;
    `,
    searchButton: css`
      flex: 1;
      display: flex;
      gap: 16px;
      justify-content: flex-end;
    `,
    dropdown: css`
      width: 106px;
      .ant-dropdown-menu-item {
        min-width: 0px;
      }
    `,
    actionButton: css`
      cursor: pointer;
      padding: 4px 6px;
      color: ${token.colorPrimary};
      &:hover {
        text-decoration: underline;
        color: ${token.colorPrimaryHover};
      }
    `,
    deleteButton: css`
      color: ${token.colorError};
    `,
    importModalContent: css`
      margin-top: 16px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    `,
    downloadTemplateButton: css`
      width: 100px;
    `,
  };
});

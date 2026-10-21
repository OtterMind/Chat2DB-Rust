import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    checkAllBox: css`
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    allTableContainer: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    spinBox: css`
      display: flex;
      justify-content: center;
      align-items: center;
      height: 0;
      flex:1;
    `,
    baseTable: css`
      height: 0px;
    `,
    supportBaseTableBoxHidden: css`
      overflow: hidden;
    `,
    table: css`
      height: 100%;
    `,
    checkboxContainer: css`
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    tableCell: css`
      height: 30px;
      line-height: 30px;
      padding: 0px 8px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: ${token.fontSize}px;
    `,
    toolBarList: css`
      display: flex;
      align-items: center;
      border-bottom: 1px solid ${token.colorBorderLayout};
    `,
    toolBarItem: css`
      flex-shrink: 0;
      height: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
      padding: 0px 4px;
      border-right: 1px solid ${token.colorBorderLayout};
    `,
    toolBarRight: css`
      flex: 1;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      padding: 0px 6px;
    `,
      searchBar: css`
      flex-shrink: 0; 
      display: flex;
      align-items: center;
      padding: 0px 6px;
      height: 26px;
      display: flex;
      border-bottom: 1px solid ${token.colorBorderLayout};
      input {
        flex: 1;
        height: 100%;
        /* Remove all Ant Design input styles. */
        padding: 0px 2px;
        border: 0;
        outline: none;
        box-shadow: none !important;
        background: none;
        color: ${token.colorText};
      }
    `,
    iconContainer: css`
      flex-shrink: 0;
      height: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
      margin-right: 2px;
    `,

    tableContent: css`
      flex: 1;
      display: flex;
      align-items: center;
      gap: 3px;
    `,
    selectBoundInfo: css`
      flex: 1;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      padding:0px 6px;
    `,
    cannotSubmit: css`
      color: ${token.colorTextSecondary};
      cursor: not-allowed;
    `,
    resultSetSearchBar: css`
      border-bottom: 1px solid ${token.colorBorderLayout};
    `,
  };
});

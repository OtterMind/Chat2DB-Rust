import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      background-color: ${token.colorBgBase};
    `,
    title: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-right: 8px;
    `,
    search: css`
      margin: 0 8px;
    `,
    flowWrapper: css`
      padding: 8px;
      margin-bottom: 20px;
      height: calc(100% - 148px);
      overflow-y: auto;
      overflow-x: hidden;
    `,
    listItem: css`
      margin: 4px 0;
    `,
    empty: css`
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      height: calc(100% - 112px);
    `,
    iconChatExcel: css`
      color: ${token.colorSuccess};
    `,
    iconChatDatabase: css`
      color: ${token.colorLink};
    `,
  };
});

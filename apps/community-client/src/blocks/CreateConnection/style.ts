import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    connectionBlock: css`
      width: 100%;
      height: 100%;
      overflow-y: auto;
    `,
    createConnections: css`
      min-height: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
      background-color: ${token.colorBgBase};
      transform: scale(0.2);
      transition: 0.1s ease-in-out;
    `,
    showCreateConnections: css`
      transform: scale(1);
      transition: transform 0.3s ease-in-out;
    `,

    dbListBox: css`
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100%;
    `,
    dbList: css`
      display: flex;
      flex-wrap: wrap;
      justify-content: center;
      gap: 20px;
      padding: 20px;
      max-width: 1200px;
    `,
  };
});

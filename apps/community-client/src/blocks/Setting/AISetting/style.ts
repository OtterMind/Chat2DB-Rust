import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    aiSqlSource: css`
      display: flex;
      margin-bottom: 20px;
    `,
    aiSqlSourceTitle: css`
      margin-right: 20px;
      min-width: 50px;
    `,
    title: css`
      font-size: 14px;
      margin-bottom: 12px;

      i {
        margin-left: 10px;
        color: ${token.colorPrimary};
      }
      & label {
        font-size: 14px !important;
      }
    `,
    content: css`
      margin-bottom: 15px;
    `,
    bottomButton: css`
      display: flex;
      justify-content: flex-end;
      margin-top: 20px;
    `,
  };
});

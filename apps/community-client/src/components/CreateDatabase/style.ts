import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    monacoEditorBox: css`
      height: 200px;
      border: 1px solid ${token.colorBorder};
      border-radius: 4px;
      overflow: hidden;
      position: relative;
    `,
    errorBox: css`
      margin-top: 12px;
    `,
    errorTitle: css`
      color: ${token.colorError};
      font-weight: 500;
    `,
    errorMessage: css`
      margin-top: 4px;
      color: ${token.colorError};
      line-height: 1.5;
      overflow-wrap: anywhere;
    `,
    previewBox: css`
      margin-bottom: 6px;
    `,
    previewText: css`
      color: ${token.colorTextSecondary};
      font-weight: 500;
    `,
    createDatabaseDom: css`
      padding-top: 20px;
    `,
  };
});

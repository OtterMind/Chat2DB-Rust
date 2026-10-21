import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    monacoEditorModal: css`
      display: flex;
      height: 400px;
      border: 1px solid ${token.colorBorder};
      border-radius: 4px;
      overflow: hidden;
    `,
    monacoEditorHeader: css`
      height: 38px;
      padding: 0px 10px;
      display: flex;
      /* justify-content: space-between; */
      gap: 8px;
      align-items: center;
      border-bottom: 1px solid ${token.colorBorder};
      color: ${token.colorTextSecondary};
    `,
    formatButton: css`
      border-radius: 3px;
      background-color: ${token.colorFillQuaternary};
      height: 24px;
      padding: 0px 7px;
      cursor: pointer;
      i {
        margin-right: 6px;
      }
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
    executeButton: css`
      color: ${token.colorSuccess};
      &:hover {
        color: ${token.colorSuccess};
      }
    `,
    monacoEditorContent: css`
      width: 0px;
      flex: 1;
      display: flex;
      flex-direction: column;
      padding: 0px -6px;
      /* border-right: 1px solid ${token.colorBorder}; */
    `,
    monacoEditor: css`
      flex: 1;
      height: 0px !important;
    `,
    result: css`
      width: 0px;
      flex: 1;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      border-left: 1px solid ${token.colorBorder};
    `,
    resultHeader: css`
      width: 100%;
      line-height: 38px;
      text-align: center;
      border-bottom: 1px solid ${token.colorBorder};
    `,
    resultContent: css`
      flex: 1;
      padding: 0px 10px;
      overflow-y: auto;
    `,
    errorTitle: css`
      display: flex;
      align-items: center;
      padding: 4px 10px;
      border-radius: 8px;
      margin-top: 10px;
      i {
        color: ${token.colorError};
        margin-right: 4px;
      }
    `,
    errorMessage: css`
      margin: 10px 0px;
      padding: 4px 10px;
      background-color: ${token.colorFillSecondary};
      border-radius: 8px;
      word-break: break-all;
    `,
  };
});

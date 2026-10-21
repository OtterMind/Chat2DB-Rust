import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      min-height: 0;
    `,
    monacoEditor: css`
      flex: 1;
      min-height: 0;
      overflow: hidden;
      display: flex;
      flex-direction: column;
    `,
    toolbar: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      justify-content: space-between;
      flex-wrap: wrap;
      gap: 8px;
      padding: 4px 8px;
      background-color: ${token.colorBgContainer};
      border-bottom: 1px solid ${token.colorBorder};
    `,
    alert: css`
      border-radius: 0;
      border-left: 0;
      border-right: 0;
    `,
    meta: css`
      flex-shrink: 0;
      padding: 4px 8px;
      color: ${token.colorTextSecondary};
      border-bottom: 1px solid ${token.colorBorderSecondary};
      font-size: 12px;
    `,
    imagePreview: css`
      flex: 1;
      min-height: 0;
      padding: 8px;
      overflow: auto;
      background: ${token.colorFillQuaternary};
      display: flex;
      align-items: center;
      justify-content: center;

      img {
        display: block;
        max-width: 100%;
        max-height: 100%;
        object-fit: contain;
      }
    `,
    editorContainer: css`
      flex: 1;
      height: 0px;
    `,
  };
});

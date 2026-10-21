import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      overflow: auto;
      padding: 12px 16px;
      background: ${token.colorBgContainer};
    `,
    empty: css`
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    list: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
    `,
    item: css`
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 8px;
      padding: 12px;
      background: ${token.colorBgElevated};
    `,
    meta: css`
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      margin-bottom: 8px;
      color: ${token.colorTextSecondary};
      font-size: 12px;
    `,
    message: css`
      margin: 0;
      white-space: pre-wrap;
      word-break: break-word;
      color: ${token.colorText};
      font-family: Menlo, Monaco, Consolas, 'Courier New', monospace;
      font-size: 12px;
      line-height: 1.6;
    `,
  };
});

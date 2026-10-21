import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    tableCell: css`
      padding: 0px 8px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
    table: css`
      height: 100%;
    `,
    copyButton: css`
      display: inline-block;
      text-align: center;
      margin-left: 6px;
      transform: translateY(2px);
    `,
    abstract: css`
      height: 100%;
      position: relative;
    `,
    item: css`
      display: flex;
      align-items: center;
      gap: 10px;
    `,
    detailSection: css`
      & + & {
        margin-top: 16px;
      }
    `,
    detailTitle: css`
      font-size: 13px;
      font-weight: 600;
      margin-bottom: 8px;
      color: ${token.colorText};
    `,
    detailBlock: css`
      margin: 0;
      padding: 12px;
      border-radius: 10px;
      background: ${token.colorFillQuaternary};
      color: ${token.colorText};
      white-space: pre-wrap;
      word-break: break-word;
      max-height: 320px;
      overflow: auto;
      font-family: Menlo, Monaco, Consolas, 'Courier New', monospace;
      font-size: 12px;
      line-height: 1.6;
    `,
  };
});

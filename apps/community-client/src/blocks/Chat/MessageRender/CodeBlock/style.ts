import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 8px;
      overflow: hidden;
      & .shiki {
        border-radius: 0px;
      }
    `,
    header: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 4px 8px;
      opacity: 0.8;
      height: 32px;
    `,
    code: css`
      min-height: 40px;
      max-height: 500px;
      overflow-y: auto;
      pre {
        border-radius: 0px !important;
      }
    `,
    codeHighlighter: css`
      border-radius: 0px !important;
      background-color: ${token.colorBgElevated} !important;
    `,
    resultWrapper: css`
      position: relative;
    `,
    result: css`
      height: 400px;
    `,
    resultFold: css`
      position: absolute;
      top: -10px;
      left: 50%;
      transform: translateX(-50%);
      display: flex;
      justify-content: center;
      align-items: center;

      border-radius: 4px;
      padding: 2px;
      cursor: pointer;
      width: 40px;
      background-color: ${token.colorBgElevated};
      &:hover {
        background-color: ${token.colorBgMask};
        box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
        transform: translateX(-50%) scale(1.05);
        transition: all 0.2s ease;
      }
    `,
  };
});

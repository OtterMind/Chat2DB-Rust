import { createStyles, keyframes } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  const showColorFlash = keyframes`
    0% {
      background-color: ${token.colorErrorBg};
    }
    50% {
      background-color: ${token.colorErrorBorderHover};
    }
    100% {
      background-color: ${token.colorErrorBg};
    }
  `;
  return {
    monacoEditorError: css`
      display: flex;
      align-items: center;
      justify-content: space-between;
      position: absolute;
      bottom: 0px;
      right: 0px;
      left: 0px;
      padding: 0px 8px;
      z-index: 100;
      color: ${token.colorError};
      font-size: 13px;
      height: 30px;
      line-height: 30px;
      animation: ${showColorFlash} 1.5s forwards;
    `,
    errorMessage: css`
      flex: 1;
      width: 0px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    `,
    closeButton: css`
      flex-shrink: 0;
    `,
  };
});

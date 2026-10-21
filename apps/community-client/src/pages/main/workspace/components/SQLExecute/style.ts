import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    boxRightCenter: css`
      height: 100%;
    `,
    boxRightConsole: css`
      height: 100%;
      overflow: hidden;
      width: 100%;
    `,
    boxRightResult: css`
      position: relative;
      height: 100%;
    `,
    tableLoading: css`
      position: absolute;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      gap: 12px;
      overflow: hidden;
    `,
    executingBar: css`
      position: absolute;
      right: 12px;
      bottom: 12px;
      z-index: 10;
      display: flex;
      align-items: center;
      gap: 8px;
      max-width: calc(100% - 24px);
      padding: 8px 12px;
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadius}px;
      background: ${token.colorBgElevated};
      box-shadow: ${token.boxShadowSecondary};
      color: ${token.colorTextSecondary};
      font-size: 12px;
      line-height: 20px;
    `,
    executingText: css`
      white-space: nowrap;
    `,
    stopExecuteSql: css`
      cursor: pointer;
      color: ${token.colorPrimary};
      &:hover {
        color: ${token.colorPrimaryHover};
      }
    `,
  };
});

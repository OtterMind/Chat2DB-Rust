import { createStyles } from 'antd-style';

export const useStyles = createStyles(
  ({ css, token }, { appTitleBarHeight, bordered }: { appTitleBarHeight: number; bordered: boolean }) => {
    return {
      panel: css`
        display: flex;
        height: calc(100vh - ${appTitleBarHeight}px);
        min-height: 0;
        overflow: hidden;
        flex-direction: column;
        box-sizing: border-box;
        background-color: ${token.colorBgBase};
        flex-shrink: 0;
        ${bordered ? `border-right: 1px solid ${token.colorBorderLayout};` : ''}
        &:focus-visible {
          outline: none;
        }
      `,
    };
  },
);

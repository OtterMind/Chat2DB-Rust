import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    ContextMenuContent: css`
      background-color: ${token.colorBgBase};
      overflow: hidden;
      padding: 4px;
      border-radius: 8px;
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
      z-index: 1100;
    `,
    ContextMenuItem: css`
      display: flex;
      align-items: center;
      justify-content: space-between;
      font-size: 14px;
      line-height: 24px;
      color: ${token.colorText};
      padding: 4px;
      border-radius: 4px;
      cursor: pointer;
      min-width: 128px;
      gap: 8px;
      &:focus {
        outline: none;
        background-color: ${token.colorFillSecondary};
      }
      &:hover {
        outline: line;
        background-color: ${token.colorFillSecondary};
      }
    `,

    ContextMenuSeparator: css`
      height: 1px;
      margin: 4px;
      background-color: ${token.colorBorderSecondary};
    `,
  };
});

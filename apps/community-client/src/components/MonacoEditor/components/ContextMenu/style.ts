import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    ContextMenuContent: css`
      background-color: ${token.colorBgBase};
      overflow: hidden;
      padding: 4px;
      border-radius: 4px;
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
      z-index: 1000;
    `,
    ContextMenuItem: css`
      font-size: 14px;
      color: ${token.colorText};
      display: flex;
      align-items: center;
      gap: 8px;
      line-height: 36px;
      padding: 0 4px;
      border-radius: 4px;
      cursor: pointer;
      min-width: 160px;
      &:focus {
        outline: none;
        background-color: ${token.colorFillSecondary};
      }
      &:hover {
        outline: line;
        background-color: ${token.colorFillSecondary};
      }
    `,

    RightSlot: css`
      margin-left: auto;
      padding-left: 20px;
      opacity: 0.5;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    item: css`
      align-items: center;
      justify-content: start;
      padding: 0px 12px;
      border-bottom: 1px solid ${token.colorBorderLayout};
      & svg {
        color: ${token.colorTextQuaternary};
      }
    `,
    dropItem: css`
      padding: 8px 10px;
      border-radius: 4px;
      &:hover {
        background-color: ${token.colorFillQuaternary};
      }
    `,
    dropItemChecked: css`
      background-color: ${token.colorFillQuaternary};
    `,
    itemLeft: css`
      display: flex;
      align-items: center;
      flex: 0 0 auto;
      min-width: 160px;
      flex: 0 0 auto;
      height: 36px;
    `,
    avatar: css`
      flex-shrink: 0;
    `,
    itemName: css`
      flex: 1;
      width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      margin-left: 12px;
      margin-right: 6px;
    `,
    itemTag: css`
      flex-shrink: 0;
      font-size: 10px;
      font-weight: ${token.fontWeightStrong};
      color: ${token.colorTextSecondary};
      border: 1px solid ${token.colorTextSecondary};
      border-radius: 2px;
      line-height: 10px;
      padding: 1px 2px;
    `,
  };
});

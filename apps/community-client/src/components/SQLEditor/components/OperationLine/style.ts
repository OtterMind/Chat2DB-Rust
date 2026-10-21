import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    consoleOptionsWrapper: css`
      flex-shrink: 0;
      height: 34px;
      display: flex;
      align-items: center;
      display: flex;
      padding: 0px 4px 0px 8px;
      justify-content: space-between;
      align-items: center;
    `,
    consoleOptionsLeft: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      gap: 6px;
      color: ${token.colorTextSecondary};
    `,
    iconButtonPlay: css`
      color: ${token.colorSuccess};
      cursor: pointer;
      &:hover {
        color: ${token.colorSuccess};
      }
    `,
    partingLine: css`
      width: 1px;
      height: 16px;
      background-color: ${token.colorBorder};
    `,
    operatingButtonIcon: css`
     
    `,
  };
});

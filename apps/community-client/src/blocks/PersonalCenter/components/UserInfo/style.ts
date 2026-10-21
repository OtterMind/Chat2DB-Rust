import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    userInfo: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      width: 240px;
      margin: 24px 0 16px 0;
    `,
    userName: css`
      display: flex;
      align-items: center;
      gap: 4px;
      margin-top: 16px;
      margin-bottom: 8px;
    `,
    displayName: css`
      font-weight: ${token.fontWeightStrong};
      line-height: 24px;

      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    foreverIcon: css`
      font-size: ${token.fontSizeLG}px;
      color: #d48e21;
    `,
  };
});

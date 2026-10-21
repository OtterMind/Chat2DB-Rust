import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css``,
    titleWrapper: css``,
    title: css`
      font-size: ${token.fontSizeLG}px;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 8px;
    `,
    titleDes: css`
      color: ${token.colorTextSecondary};
    `,
    inviteWrapper: css`
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 8px 12px;
      border-radius: 6px;
      background-color: ${token.colorBorderSecondary};
      font-weight: ${token.fontWeightStrong};
    `,

    amountWrapper: css`
      display: flex;
      justify-content: space-around;
      border-top: 1px solid ${token.colorBorderSecondary};
      border-bottom: 1px solid ${token.colorBorderSecondary};
      margin-top: 20px;
      margin-bottom: 48px;
    `,

    amountItem: css`
      margin: 42px 0;
      padding: 0 48px;
      display: flex;
      flex-direction: column;
      align-items: center;
      flex: 1;
      &:not(:last-child) {
        border-right: 1px solid ${token.colorBorderSecondary};
      }
    `,
    amountCount: css`
      font-size: 24px;
      font-weight: ${token.fontWeightStrong};
    `,
    amountTitle: css`
      white-space: nowrap;
    `,
    inviteListWrapper: css``,
    inviteTitle: css`
      font-size: ${token.fontSizeLG}px;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 8px;
      display: flex;
      align-items: center;
      gap: 12px;
    `,

    inviteListWrapper: css``,
  };
});

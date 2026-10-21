import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      margin-top: 20px;
    `,
    initItem: css`
      border-radius: 12px;
      border: 1px solid ${token.colorBorderSecondary};
      background-color: ${token.colorFillQuaternary};
      padding: 32px 24px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      cursor: pointer;
      margin-bottom: 20px;
      &:hover {
        background-color: ${token.colorFillSecondary};
      }
    `,
    initItemIcon: css`
      width: 36px;
      height: 36px;
      border-radius: 6px;
      background-color: ${token.colorPrimary};
      display: flex;
      justify-content: center;
      align-items: center;
      color: ${token.colorPrimaryBg};
    `,
    initItemIconSuccess: css`
      background-color: ${token.colorSuccess};
    `,

    initItemTitle: css`
      font-weight: ${token.fontWeightStrong};
      font-size: 16px;
    `,
    initItemDesc: css`
      font-size: 12px;
      color: ${token.colorTextTertiary};
    `,
    initItemArrow: css`
      color: ${token.colorTextQuaternary};
    `,

    inviteWrapper: css`
      /* margin: 40px 0; */
      width: 80%;
      padding: 20px;
      border-radius: 12px;
      border: 1px solid ${token.colorBorderSecondary};
      background-color: ${token.colorFillQuaternary};
    `,
  };
});

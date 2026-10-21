import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    teamSeat: css``,
    modalContent: css`
      background: ${token.colorBgContainer};
      padding: 24px 28px 24px;
      font-family: ${token.fontFamily};
    `,
    modal: css`
      .ant-modal-close {
        top: 22px;
        right: 22px;
        border-radius: 999px;
        background: ${token.colorFillQuaternary};
      }
      .ant-modal-body {
        overflow: hidden;
        max-height: none;
        padding-block: 0 !important;
      }
    `,
    modalHeader: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
      margin-bottom: 18px;
      padding-right: 42px;
    `,
    modalEyebrow: css`
      font-size: 13px;
      line-height: 18px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      color: ${token.colorTextSecondary};
      font-weight: 700;
    `,
    modalTitle: css`
      font-size: 26px;
      line-height: 32px;
      font-weight: 700;
      color: ${token.colorText};
      font-family: ${token.fontFamily};
    `,
    payBlockTop: css`
      border-bottom: 1px dashed ${token.colorBorderSecondary};
      padding-bottom: 20px;
      margin-bottom: 20px;
    `,
    topSubTitle: css`
      margin-top: 4px;
      max-width: 320px;
      font-size: 12px;
      line-height: 18px;
      color: ${token.colorTextTertiary};
    `,
    bottom: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 18px;
    `,
    bottomLeft: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
    `,
    money: css`
      font-size: 18px;
      font-weight: 700;
      color: ${token.colorText};
    `,
    button: css`
      height: 44px;
      padding: 0 24px !important;
      border-radius: 999px;
      border-width: 0;
      background: ${token.colorPrimary};
      box-shadow: 0 12px 32px ${token.colorBgMask};
    `,
    inviteCode: css`
      display: flex;
      align-items: center;
      gap: 8px;
      font-size: 12px;
      & > span {
        color: ${token.colorTextTertiary};
      }
      & > input {
        width: 100px;
        height: 26px;
      }
      & > .error {
        color: ${token.colorError};
      }
      & svg {
        color: ${token.colorSuccess};
      }
    `,
  };
});

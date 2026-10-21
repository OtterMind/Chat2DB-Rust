import { createStyles, keyframes } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  // Shake around the element's upper center point.
  const shake = keyframes`
  0% {
    transform: rotate(0deg);
  }
  25% {
    transform: rotate(8deg);
  }
  50% {
    transform: rotate(0deg);
  }
  75% {
    transform: rotate(-8deg);
  }
  100% {
    transform: rotate(0deg);
  }
  `;

  return {
    versionText: css`
      color: ${token.colorPrimary};
    `,
    notificationBtnBox: css`
      display: flex;
      align-items: center;
      justify-content: space-between;
    `,
    updateReminder: css`
      display: flex;
      align-items: center;
      font-size: 14px;
      i {
        font-size: 18px;
        color:  ${token.colorPrimary};
      }
    `,
    bell: css`
      animation: ${shake} 0.2s linear 10;
      margin-right: 4px;
    `,
    btnBox: css`
      display: flex;
      justify-content: flex-end;
    `,
    notification: css`
      z-index: 999 !important;
      font-size: 12px !important;
      padding: 12px 12px !important;
      .ant-notification-notice-close {
        top: 12px !important;
        inset-inline-end: 8px !important;
      }
      .ant-notification-notice-message{
        margin-bottom: 0px !important;
      }
    `,
  };
});

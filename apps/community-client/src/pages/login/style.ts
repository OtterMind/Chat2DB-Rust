import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    loginBox: css`
      width: 460px;
      margin: 0 auto;
      padding: 40px;
      border-radius: 10px;
      background-color: ${token.colorBgContainer};
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
    `,
    welcomeBack: css`
      font-size: 24px;
      font-weight: bold;
      text-align: center;
      margin-bottom: 10px;
    `,
    subheading: css`
      font-size: 14px;
      text-align: center;
      margin-bottom: 30px;
      color: ${token.colorTextSecondary};
    `,
    thirdPartyLoginButton: css`
      display: flex;
      align-items: center;
      justify-content: center;
      width: 100%;
      margin-bottom: 20px;
    `,
    icon: css`
      height: 20px;
      width: 20px;
      margin-right: 10px;
      fill: ${token.colorBgBase};
    `,
    sendCode: css`
      margin-bottom: 16px;
      button:hover {
        background-color: transparent;
        color: ${token.colorPrimary};
      }
    `,
    codeCountDownTips: css`
      padding: 0px 2px;
      color: ${token.colorTextSecondary};
    `,
    forgetPasswordTips: css`
      margin-bottom: 4px;
      background-color: ${token.colorPrimaryBg};
      padding: 0px 10px;
      border-radius: 8px;
      height: 0px;
      transition: height 0.15s;
      transform-origin: 100%;
      overflow: hidden;
    `,
    activeForgetPasswordTips: css`
      padding: 4px 10px;
      height: 48px;
      line-height: 24px;
      transition: height 0.15s;
      transform-origin: 100%;
    `,
    emailLoginFooter: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 16px;
      user-select: none;
      padding: 0px 2px;
    `,
    tipsByLink: css`
      color: ${token.colorTextSecondary};
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
        text-decoration: underline;
      }
    `,
    loginButton: css`
      width: 100%;
    `,
    agreement: css`
      font-size: 12px;
      text-align: center;
      a {
        color: ${token.colorLink};
        margin: 0px 4px;
        &:hover {
          color: ${token.colorLinkHover};
        }
      }
    `,
    loginTip: css`
      text-align: center;
      line-height: 32px;
      `,
      wechatSpan: css`
        color: #63b039;
        margin: 0px 2px;
        `,
    wechatBox: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      margin-bottom: 30px;
    `,
    qrcodeWrapper: css`
      margin-top: 12px;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      position: relative;
      padding: 2px;
      border: 1px solid ${token.colorBorder};
      border-radius: 8px;
      width: 208px;
      height: 208px;
    `,
    loginQRCode: css`
      width: 100%;
      height: 100%;
    `,
    loginQRCodeMask: css`
      position: absolute;
      left: 0;
      right: 0;
      top: 0;
      bottom: 0;
      background-color: ${token.colorBgMask};
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      color: #fff;
      font-size: 14px;
      line-height: 36px;
      cursor: pointer;
      font-weight: 500;
    `,
    wechatLoginButton: css`
      color: #63b039;
      i {
        margin-right: 10px;
      }
      &:hover {
        color: #63b039 !important;
        border: 1px solid #63b039 !important;
        filter: brightness(1.1);
      }
    `,
  };
});

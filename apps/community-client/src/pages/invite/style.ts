import { createStyles } from 'antd-style';
import inviteImage from '@/assets/img/bg.webp';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      height: 100vh;
      width: 100vw;
      display: flex;
      justify-content: center;
      align-items: center;
      background-image: url(${inviteImage});
      background-size: cover;
      background-position: center center;
      background-repeat: no-repeat;
      /* background-color: ${token.colorBorder}; */
      /* background-image: url(${inviteImage}); */
    `,
    cardWrapper: css`
      height: 720px;
      border-radius: 24px;
      background-color: ${token.colorBgBase};
      box-shadow: ${token.boxShadow};
      padding: 0 40px;
      display: flex;
      flex-direction: column;
    `,
    headerWrapper: css`
      position: relative;
      padding: 80px 80px 50px 80px;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 4px;
      border-bottom: 1px dotted ${token.colorBorder};
      & > span {
        font-size: 16px;
        line-height: 24px;
        color: ${token.colorTextTertiary};
      }
    `,

    // leftBall: css`
    //   position: absolute;
    //   bottom: -15px;
    //   left: -55px;
    //   width: 30px;
    //   height: 30px;
    //   border-radius: 50%;
    //   background-color: ${token.colorBorder};
    // `,
    // rightBall: css`
    //   position: absolute;
    //   bottom: -15px;
    //   right: -55px;
    //   width: 30px;
    //   height: 30px;
    //   border-radius: 50%;
    //   background-color: ${token.colorBorder};
    // `,

    contentWrapper: css`
      padding: 40px 40px 20px 40px;
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      align-items: center;
      flex: 1;
    `,
    content: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      /* width: 80%; */
    `,
    contentText: css`
      font-weight: ${token.fontWeightStrong};
      font-size: 24px;
      line-height: 1.8;
      margin-bottom: 32px;
      display: flex;
      flex-direction: column;
      align-items: center;
      /* letter-spacing:2px; */
    `,
    highlight: css`
      color: ${token.colorPrimary};
    `,
  };
});

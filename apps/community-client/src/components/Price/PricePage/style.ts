import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    pricePage: css`
      width: 100vw;
      height: 100vh;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
    `,
    priceTitle: css`
      margin-bottom: 24px;
      font-size: 24px;
      font-weight: 600;
    `,
    priceContent: css`
      display: flex;
      min-width: 720px;
      max-width: 1200px;
      border-radius: 16px;
      border: 1px solid ${token.colorBorderSecondary};
      overflow: hidden;
    `,
    priceIntro: css`
      padding-top: 20px;
    `,
  };
});

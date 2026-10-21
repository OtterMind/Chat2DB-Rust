import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      display: flex;
      align-items: flex-start;
      padding: 4px 8px;
      margin-bottom: 8px;
      gap: 12px;
      .timeBox {
        position: absolute;
        top: -16px;
        font-size: 12px;
        color: ${token.colorTextSecondary};
        line-height: 12px;
        margin-bottom: 4px;
        display: none;
      }
      .avatarBox {
        display: flex;
        align-items: center;
        height: 40px;
        width: 40px;
        border-radius: 50%;
        overflow: hidden;
      }
      .avatarBoxLogo {
        background-color: ${token.colorBgBase};
      }
      .messageContainer {
        position: relative;
        width: 100%;
        /* min-height: 44px; */
        align-items: center;
      }
      .messageContent {
        display: flex;
        flex-direction: row-reverse;
        gap: 10px;
        max-width: 100%;
        width: 100%;
      }
      .actionBarBox {
        width: 100px;
        flex-shrink: 0;
      }
      .messageBox {
        padding: 8px 10px;
        background-color: ${token.colorBgContainer};
        border-radius: 8px;
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        gap: 4px;
        width: 100%;
      }
      &:hover {
        .timeBox {
          display: block;
        }
      }
    `,
    questionContainer: css`
      border-radius: 4px;
      background-color: ${token.colorBgContainer};
      .messageContent {
        flex-direction: row;
      }
      .timeBox {
        right: 0;
      }
    `,
    answerContainer: css`
      /* padding: 0; */
      .messageBox {
        padding: 0;
        background-color: transparent;
      }
    `,
  };
});

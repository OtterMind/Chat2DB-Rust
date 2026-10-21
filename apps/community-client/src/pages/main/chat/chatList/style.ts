import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css`
      flex: 1;
      height: 0px;
      position: relative;
      `,
    chatScroll: css`
      height: 100%;
      overflow-y: auto;
      box-sizing: border-box;
      padding: 8px 8px 8px 8px;
      display: flex;
      flex-direction: column-reverse;
    `,
    oneRoundChat: css`
      margin-bottom: 20px;
    `,
    chatItemFooter: css`
      margin-top: 4px;
      display: flex;
      flex-direction: column;
      gap: 4px;
    `,
    centerDiv: css`
      text-align: center;
      line-height: 18px;
    `,
  };
});

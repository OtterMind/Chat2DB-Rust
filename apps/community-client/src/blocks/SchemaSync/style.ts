import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      box-sizing: border-box;
      padding-top: 20px;
    `,
    body: css`
      margin-bottom: 20px;
    `,
    title: css`
      font-size: 26px;
      text-align: center;
    `,
    monacoEditor: css`
      position: relative;
      height: 100%;
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 4px;
      height: 60vh;
      overflow: hidden;
    `,
    hiddenStep: css`
      display: none;
    `,
    abstractBox: css`
      height: 350px;
      /* border: 1px solid ${token.colorBorderSecondary}; */
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    a: css``,
    editChartCard: css`
      display: flex;
      height: calc(100% - 2px);
    `,
    left: css`
      /* flex: 35; */
      flex: 1;
      width: 0px;
      /* padding: 20px; */
      box-sizing: border-box;
    `,
    right: css`
      /* flex: 15; */
      display: flex;
      flex-direction: column;
      width: 350px;
      height: 100%;
      flex-shrink: 0;
      box-sizing: border-box;
      /* max-width: 300px; */
      border-left: 1px solid ${token.colorBorderLayout};
    `,
    formBox: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    formContent: css`
      flex: 1;
      height: 0px;
      overflow-y: auto;
      padding: 16px;
    `,
    buttonBox: css`
      flex-shrink: 0;
      height: 40px;
      display: flex;
      justify-content: center;
      button {
        width: 90%;
      }
    `,
  };
});

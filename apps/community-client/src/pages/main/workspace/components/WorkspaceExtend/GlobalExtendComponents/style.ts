import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    viewDDLBox: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    viewDDLHeader: css`
      flex-shrink: 0;
      line-height: 40px;
      height: 36px;
      padding: 0px 10px;
      border-bottom: 1px solid ${token.colorBorder};
      font-weight: bold;
      box-sizing: border-box;
    `,
    viewDDL: css`
      flex: 1;
    `,
    noInformation: css`
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    grantsSpin: css`
      flex: 1;
      min-height: 0;

      .ant-spin-container {
        height: 100%;
      }
    `,
    grantsContent: css`
      height: 100%;
      padding: 10px;
      overflow: auto;
      box-sizing: border-box;
    `,
  };
});

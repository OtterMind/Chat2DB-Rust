import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    terminalContainerBox: css`
      display: flex;
      flex-direction: column;
      padding: 8px 10px 0px;
      border-radius: 4px;
      overflow: hidden;
      background-color: #24292f;
      color: #f6f8fa;
      height: 100%;
      box-sizing: border-box;
    `,
    terminalContainer: css`
      height: 0px;
      flex: 1;
    `,
  };
});

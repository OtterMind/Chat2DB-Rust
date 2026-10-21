import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    generatingOrder: css`
      height: 100vh;
      width: 100vw;
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 10px;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    wrapper: css`
      position: relative;
      height: 100%;
      overflow: hidden;
    `,
    chartContainer: css`
      width: 100%;
      height: 100%;
    `,
  };
});

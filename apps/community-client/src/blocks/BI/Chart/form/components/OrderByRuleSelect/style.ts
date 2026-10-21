import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      align-items: center;
    `,
    colorStripe: css`
      display: flex;
      width: 220px;
      border-radius: 5px;
      height: 12px;
      overflow: hidden;
      align-items: center;
    `,
    colorStripeItem: css`
      flex: 1;
      height: 100%;
    `,
  };
});

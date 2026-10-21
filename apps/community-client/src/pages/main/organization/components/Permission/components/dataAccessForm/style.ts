import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    formWrapper: css`
      padding: 10px 32px;
    `,
    dataSourceWrapper: css`
      display: flex;
      justify-content: start;
      align-items: center;
    `,
  };
});

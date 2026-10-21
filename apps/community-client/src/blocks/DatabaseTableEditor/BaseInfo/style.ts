import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    baseInfo: css`
      padding: 20px 10px 0px;
      display: flex;
      height: 100%;
    `,
    formBox: css`
      width: 50%;
    `,
  };
});

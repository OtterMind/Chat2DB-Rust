import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    chatBlankPage: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
    `,
    text: css`
      font-size: 16px;
      font-weight: 500;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    questionTips: css`
      margin-left: 2px;
      color: ${token.colorTextSecondary};
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
  };
});

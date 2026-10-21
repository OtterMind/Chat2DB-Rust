import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    copyContainer: css`
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
        text-decoration: underline;
      }
    `,
  };
});

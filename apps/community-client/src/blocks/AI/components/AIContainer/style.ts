import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      background-color: ${token.colorBgLayout};
      display: flex;
      flex-direction: column;
    `,
  };
});

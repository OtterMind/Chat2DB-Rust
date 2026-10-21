import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      background-color: ${token.colorBgBase};
    `,
    listWrapper: css`
      height: calc(100% - 48px);
      overflow-y: auto; 
    `,
  };
});

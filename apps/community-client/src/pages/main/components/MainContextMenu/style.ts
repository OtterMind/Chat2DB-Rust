import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    popup: css`
      position: fixed;
      z-index: ${token.zIndexPopupBase + 10};
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    exportBar: css`
      cursor: pointer;
      color: ${token.colorTextSecondary};
      font-size: 13px;
    `,
  };
});

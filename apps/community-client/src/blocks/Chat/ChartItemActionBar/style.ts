import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    chartItemActionBar: css`
      width: 100%;
      display: flex;
      gap: 4px;
    `,
    activeIcon: css`
      color: ${token.colorPrimary};
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
  };
});

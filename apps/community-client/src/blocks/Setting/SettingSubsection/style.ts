import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    subsection: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
      margin-bottom: 15px;
    `,
    title: css`
      font-weight: 500;
      font-size: 16px;
    `,
    describe: css`
      color: ${token.colorTextSecondary};
    `,
    divider: css`
      margin: 10px 0px 0px 0px;
    `,

  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {

  return {
    footer: css`
      display: flex;
      align-items: center;
    `,
    footerLeft: css`
      display: flex;
      align-items: center;
      flex: 1;
      gap: 8px;
    `,
    footerRight: css`
      display: flex;
      align-items: center;
      width: fit-content;
      flex-shrink: 0;
      gap: 8px;
    `
  };
});

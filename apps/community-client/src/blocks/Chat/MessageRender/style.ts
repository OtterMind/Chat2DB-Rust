import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    message: css`
      max-width: 100%;
    `,
        chatItemFooter: css`
        margin-top: 4px;
        display: flex;
        flex-direction: column;
        gap: 4px;
      `,

  };
});

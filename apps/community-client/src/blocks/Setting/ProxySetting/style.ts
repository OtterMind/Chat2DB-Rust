import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    title: css`
      font-size: 14px;
      margin-bottom: 10px;

      i {
        margin-left: 10px;
        color: var(--color-primary);
      }
    `,
    content: css`
      margin-bottom: 15px;
    `,
    bottomButton: css`
      display: flex;
      justify-content: flex-end;
      margin-top: 20px;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
      .ant-form-item-label{
        width: 120px;
      }
    `,
    title: css`
      font-size: 14px;
      font-weight: 500;
      color: ${token.colorText};
      text-align: center;
    `,
    enumContainer: css`
      display: flex;
      gap: 10px;
    `,
    enumItem: css`
      flex: 1;
    `,
  };
});

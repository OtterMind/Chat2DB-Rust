import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    checkboxBody: css`
      .ant-form-item {
        margin-bottom: 0;
      }
      .ant-form-item-control-input{
        min-height: 30px;
      }
    `,
  };
});

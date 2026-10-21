import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css`

    `,
    smallForm: css`
      .ant-form-item-label {
        padding: 0px 0px 4px !important;
        label {
          font-size: 13px;
        }
      }
      .ant-form-item {
        margin-bottom: 6px !important;
      }
    `,
    dropdown: css`
      .ant-select-item {
        font-size: 13px;
        padding: 0px 6px !important;
        min-height: 28px !important;
        line-height: 28px !important;
      }
    `,
    emptyLabel: css`
      height: 0px !important;
    `,
  };
});

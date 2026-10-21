import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css``,

    formItem: css`
      margin-bottom: 0px !important;
      height: 30px !important;
      display: flex;
      align-items: center;
      .ant-form-item-row{
        width: 100%;
      }
      .ant-form-item-control-input {
        height: 30px !important;
        min-height: 30px !important;
      }
    `,
    tableCell: css`
      height: 30px !important;
      box-sizing: border-box;
      border-radius: 0px !important;
      padding: 4px 6px !important;
      border: 1px solid transparent !important;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      span {
        display: inline-block;
        height: 100%;
        width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    `,
    input: css`
      height: 30px !important;
      box-sizing: border-box;
      border-radius: 0px !important;
      padding: 4px 6px !important;
    `,
    select: css`
      height: 30px !important;
      box-sizing: border-box;
      border-radius: 0px !important;
      padding: 4px 6px !important;
    `,
    checkbox: css`
      margin-left: 6px;
    `,
  };
});

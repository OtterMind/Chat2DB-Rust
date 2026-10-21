import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    editData: css`
      padding: 0px 20px;
      box-sizing: border-box;
      height: 100%;
    `,
    form: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      .ant-form-item-label label{
        user-select: none;
      }
    `,
    firstLine: css`
      display: flex;
      gap: 20px;
      flex-wrap: wrap;
    `,
    nameFormItem: css`
      width: 240px;
    `,
    typeFormItem: css`
      width: 150px;
    `,
    ttlFormItem: css`
      width: 200px;
    `,
    textAreaFormItem: css`
      height: 100%;
    `,
    createList: css`
      height: 100%;
      .ant-form-item {
        height: calc(100% - 16px);
      }
      .ant-form-item-label {
        flex-shrink: 0;
      }
      .ant-form-item-control {
        flex: 1;
      }
      .ant-row {
        flex-wrap: nowrap;
      }
      .ant-form-item-row,.ant-form-item-control-input,.ant-form-item-control-input-content {
        height: 100%;
      }
    `,
    fullFormItemBox: css`
      flex: 1;
      height: 0px;
      overflow: hidden;
      .ant-form-item {
        height: calc(100% - 16px);
      }
      .ant-form-item-label {
        flex-shrink: 0;
      }
      .ant-form-item-control {
        flex: 1;
      }
      .ant-row {
        flex-wrap: nowrap;
      }
      .ant-form-item-row,.ant-form-item-control-input,.ant-form-item-control-input-content {
        height: 100%;
        
        textarea {
          height: 100% !important;
        }
      }
    `,
  };
});

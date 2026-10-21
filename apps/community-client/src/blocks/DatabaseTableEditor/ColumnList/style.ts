import { createStyles } from 'antd-style';
import { createVar } from '@/styles/var';

export const useStyles = createStyles(({ css, token }) => {
  const varStyles = createVar(token);
  return {
    container: css`
      height: 100%;
      padding: 10px;
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      .ant-table-body {
        border: 1px solid ${token.colorBorder};
        border-top: 0px;
        border-bottom: 0px;
      }
      .ant-table-header {
        border: 1px solid ${token.colorBorder};
        border-bottom: 0px;
        user-select: none;
      }
      .ant-table {
        border-radius: 10px;
        border-bottom: 0px;
      }
      .ant-table-wrapper .ant-table-tbody > tr > td {
        // border: 0px;
        padding: 4px 2px;
      }
      .ant-table-wrapper .ant-table-thead > tr > th {
        padding: 8px 4px;
        &::before {
          display: none;
        }
      }
      .ant-table-wrapper .ant-table-thead > tr > td {
        &::before {
          display: none;
        }
      }
      // antd cannot set the minimum width, so set the minimum column width here to 100px
      colgroup col:nth-last-child(2) {
        min-width: 100px;
      }

      .react-resizable {
        position: relative;
        background-clip: padding-box;
      }

      .react-resizable-handle {
        position: absolute;
        width: 10px;
        height: 100%;
        bottom: 0;
        right: -5px;
        cursor: col-resize;
        z-index: 1;
        background-image: none;
        &:hover {
          background-color: ${token.colorFillSecondary};
        }
      }
    `,
    containerHeader: css`
      margin: 0px -5px 10px;
      flex-shrink: 0;
      button {
        margin: 0px 5px;
      }
    `,
    formBox: css`
      height: 0px;
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: 10px;
    `,
    tableBox: css`
      flex: 1;
      display: flex;
      flex-direction: column;
      border-radius: 8px 8px 0px 0px;
      overflow: hidden;
    `,
    addColumnButton: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      border: 1px dashed ${token.colorBorder};
      line-height: 30px;
      margin-top: 10px;
      color: ${token.colorTextSecondary};
      cursor: pointer;
      user-select: none;
      i {
        margin-right: 5px;
      }
      &:hover {
        color: ${token.colorPrimary};
        border-color: ${token.colorPrimary};
      }
    `,
    otherInfo: css`
      flex-shrink: 0;
      padding: 10px;
      box-sizing: border-box;
      height: 200px;
      display: flex;
      align-items: center;
      justify-content: center;
      border-top: 1px solid ${token.colorBorder};
    `,
    otherInfoFormBox: css`
      min-height: 140px;
      width: 400px;
    `,
    editableCell: css`
      height: 30px;
      line-height: 30px;
      padding: 0px 7px;
      box-sizing: border-box;
      border: 1px solid transparent;
      ${varStyles.singleLine}
      width: 100%;
      cursor: pointer;
    `,
    keyBox: css`
      position: relative;
      width: 30px;
      height: 30px;
      line-height: 30px;
      display: flex;
      justify-content: center;
      align-items: center;
      cursor: pointer;
      i {
        color: ${token.colorWarning};
      }
      span {
        position: absolute;
        font-weight: bold;
        right: 4px;
        bottom: -2px;
        transform: scale(0.8);
      }
    `,
    disabledKeyBox: css`
      cursor: default;
    `,
    operationBar: css`
      display: flex;
      justify-content: end;
    `,
    deleteIconBox: css`
      height: 26px;
      width: 26px;
      display: flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
    columnListCell: css`
      span {
        margin-right: 8px;
        color: ${token.colorPrimary};
        cursor: pointer;
        &:hover {
          color: ${token.colorPrimaryHover};
        }
      }
    `,
    columnNameInput: css``,
    inputNumber: css`
      width: 100%;
      max-width: 90px;
    `,
    checkboxContainer: css`
      .ant-form-item-control-input-content{
        display: flex;
        align-items: center;
        padding-left: 100px;
      }
    `,
  };
});

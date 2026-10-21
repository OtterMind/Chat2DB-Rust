import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css,token }) => {
  return {
    container: css``,
    questionTips: css`
    margin-left: 2px;
    color: ${token.colorTextSecondary};
    cursor: pointer;
    &:hover {
      color: ${token.colorPrimary};
    }
    
  `,
    
    excelTips: css`
    `,
    chatCornerstoneTypeItemList: css`
      display: flex;
      margin-top: 16px;
      gap: 12px;
    `,
    chatCornerstoneTypeItem: css`
      flex: 1;
      gap: 12px;
      display: flex;
      align-items: center;
      border-radius: 8px;
      border: 1px solid ${token.colorBorderSecondary};
      padding: 20px 16px;
      cursor: pointer;
    `,
    iconBox: css`
      height: 36px;
      width: 36px;
      border-radius: 6px;
    `,
    chatCornerstoneTypeItemActive: css`
      border: 1px solid ${token.colorPrimary};
    `,
    rightBox: css`
      display: flex;
      flex-direction: column;
    `,
    typeTitle: css``,
    typeDescription: css`
      color: ${token.colorTextSecondary};
    `,
    excelIconBox: css`
      height: 36px;
      width: 36px;
      border-radius: 6px;
      background-color: ${token.colorSuccess};
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    databaseIconBox: css`
      height: 36px;
      width: 36px;
      border-radius: 6px;
      background-color: ${token.colorLink};
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    icon: css`
      color: ${token.colorWhite};
    `,
    uploadLocalFile: css`
      margin-top: 16px;
    `,
    selectData: css`
      margin-top: 16px;
    `,
    excelTitleBox: css`
      display: flex;
      align-items: center;
      gap: 4px;
    `,
    hotIconBox: css`
      background: linear-gradient(to right, #FFC634 0%, #FF6433 100%);
      background-clip: text;
      -webkit-text-fill-color: transparent;
    `,
    checkExcelModal: css`
      .ant-modal-body{
        max-height: none;
        padding: 0px !important;
        overflow: hidden;
      }
      >div {
        height: 100%;
      }
    `,
  };
});

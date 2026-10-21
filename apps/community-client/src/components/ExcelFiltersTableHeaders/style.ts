import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    excelFiltersTableHeaders: css`
      display: flex;
      height: 100%;
    `,
    excelPreview: css`
      flex: 1;
      width: 0px;
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    actionBar: css`
      width: 310px;
      padding: 30px 20px 24px;
      border-left: 1px solid ${token.colorBorderLayout};
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      .ant-form-item-label > label {
        font-weight: 400;
      }
      .ant-form-item {
        padding: 20px 8px;
        margin: 0px;
        border-bottom: 1px solid ${token.colorBorderLayout};
      }
      .ant-form-item:last-child {
        border-bottom: none;
      }
    `,
    actionBarContent: css`
      flex: 1;
    `,
    functionDescription: css`
      display: flex;
      flex-direction: column;
      gap: 8px;
      padding-bottom: 20px;
      border-bottom: 1px solid ${token.colorBorderLayout};
      .title {
        font-size: 16px;
        line-height: 24px;
      }
      .description {
        font-size: 12px;
        line-height: 16px;
        color: ${token.colorTextSecondary};
      }
    `,
    indexColumns: css`
      height: 31px;
      line-height: 31px;
      text-align: center;
      background-color: ${token.colorFillQuaternary};
    `,
    cellColumns: css`
      height: 31px;
      line-height: 31px;
      background-color: ${token.colorFillQuaternary};
      padding: 0px 8px;
    `,
    excelPreviewContent: css`
      flex: 1;
      height: 0px;
      position: relative;
    `,
    sheetSwitch: css`
      height: 44px;
      width: 100%;
      padding: 0px;
      border-top: 1px solid ${token.colorBorderLayout};
      .ant-tabs-nav-more {
        display: none;
      }
      .ant-tabs-nav-wrap::before {
        display: none;
      }
      .ant-tabs-nav-wrap::after {
        display: none;
      }
      .ant-tabs .ant-tabs-tab+.ant-tabs-tab {
        margin-left: 0px;
      }
      .ant-tabs-tab {
        padding: 0px 20px;
        line-height: 44px;
        border-right: 1px solid ${token.colorBorderLayout};
      }
      .ant-tabs-ink-bar{
        top: 0px;
      }
      
    `,
    sheetSwitchItem: css`
      padding: 0px 20px;
      line-height: 44px;
      width: fit-content;
    `,
    sheetSwitchLastNext: css`
      display: flex;
      /* justify-content: space-between; */
      margin-top: 10px;
      .last,.next{
        cursor: pointer;
        color: ${token.colorPrimary};
        user-select: none;
        &:hover{
          /* text-decoration: underline; */
          color: ${token.colorPrimaryHover};
        }
      }
    `,
  };
});

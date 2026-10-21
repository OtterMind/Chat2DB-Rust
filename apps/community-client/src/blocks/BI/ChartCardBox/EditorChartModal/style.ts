import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    createDashboardList: css`
      display: flex;
      flex-wrap: wrap;
      justify-content: center;
      gap: 20px;
      padding: 20px;
      max-width: 1200px;
    `,
    editDashboardModal: css`
      .ant-modal-header {
        margin-bottom: 0px !important;
        height: 54px !important;
        line-height: 54px !important;
      }
      .ant-modal-title {
        width: 100%;
      }
      .ant-modal-content {
        padding: 0px !important;
        height: 88vh;
      }
    `,
    editDashboardModalTitle: css`
      min-height: 28px;
      height: 28px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 20px;
      padding: 0px 16px;
    `,
    editDashboardWrapper: css`
      height: calc(100% - 20px);
      border-radius: 10px;
      overflow: hidden;
      display: flex;
      flex-direction: column;
      border: 1px solid ${token.colorBorderLayout};
      box-sizing: border-box;
    `,
    editDashboardBody: css`
      flex: 1;
      height: 0px;
    `,
    editorChart: css`
      display: none !important;
      `,
    editorChartSql: css`
      display: none !important;
    `,
    editText: css`
      height: 28px;
      flex: 1;
      width: 0px;
    `,
    dataConfiguration: css`
      flex-shrink: 0;
      font-size: 14px;
      font-weight: 400;
    `,
    back: css`
      height: 24px;
      line-height: 24px;
      display: flex;
      align-items: center;
      width: fit-content;
      user-select: none;
      padding: 2px 10px;
      border-radius: 4px;
      cursor: pointer;
      font-size: 14px;
      background: ${token.colorPrimaryBgHover};
      &:hover {
        color: ${token.colorPrimary};
      } 
      span {
        margin-left: 4px;
      }
    `,
  };
});

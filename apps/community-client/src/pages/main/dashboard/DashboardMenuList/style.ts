import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      overflow: hidden;
    `,
    header: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      flex-shrink: 0;
      height: 32px;
      padding: 0 8px 0 20px;
      font-size: 12px;
      color: ${token.colorTextTertiary};
      white-space: nowrap;
    `,
    flowWrapper: css`
      flex: 1;
      min-height: 0;
      overflow-y: auto;
      overflow-x: hidden;
      padding: 0 8px;
    `,
    dashboardItem: css`
      position: relative;
      display: flex;
      align-items: center;
      padding: 8px 12px;
      border-radius: 8px;
      cursor: pointer;
      transition: background-color 0.15s;
      &:hover {
        background-color: ${token.colorFillTertiary};
      }
      &:hover .dashboard-item-delete {
        opacity: 1;
      }
    `,
    dashboardItemActive: css`
      background-color: ${token.colorPrimaryBg};
    `,
    dashboardItemTitle: css`
      font-size: 13px;
      color: ${token.colorText};
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      flex: 1;
      min-width: 0;
      line-height: 20px;
    `,
    dashboardItemDelete: css`
      opacity: 0;
      transition: opacity 0.15s;
      flex-shrink: 0;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    empty: css`
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      flex: 1;
      min-height: 0;
    `,
  };
});

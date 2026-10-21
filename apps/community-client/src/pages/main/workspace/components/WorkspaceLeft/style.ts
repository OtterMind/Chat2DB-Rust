import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    treeBox: css`
      flex: 1;
      padding-left: 6px;
    `,
    panelPane: css`
      display: none;
      min-height: 0;
      flex: 1;
      flex-direction: column;
    `,
    panelPaneActive: css`
      display: flex;
      min-height: 0;
      flex: 1;
      flex-direction: column;
    `,
    resourceSwitcher: css`
      display: flex;
      align-items: center;
      flex-shrink: 0;
      height: 42px;
      padding: 0 16px;
      border-bottom: 1px solid ${token.colorBorderLayout};
    `,
    resourceTabs: css`
      display: flex;
      align-items: center;
      gap: 18px;
      height: 100%;
      min-width: 0;
    `,
    resourceTitle: css`
      position: relative;
      height: 100%;
      padding: 0;
      border: none;
      background: transparent;
      color: ${token.colorTextTertiary};
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      transition: color 0.15s;

      &:hover {
        color: ${token.colorText};
      }

      &::after {
        content: '';
        position: absolute;
        left: 0;
        right: 0;
        bottom: 0;
        height: 2px;
        border-radius: 2px 2px 0 0;
        background: transparent;
      }
    `,
    resourceTitleActive: css`
      color: ${token.colorText};
      font-weight: 600;

      &::after {
        background: ${token.colorPrimary};
      }
    `,
    noConnectionList: css`
      height: 100%;
      margin-top: 30vh;
      text-align: center;
      font-size: 14px;
    `,
    noConnectionListIcon: css`
      font-size: 60px;
      color: ${token.colorPrimary};
    `,
    noConnectionListTips: css`
      margin: 10px 0px;
    `,
    create: css`
      color: ${token.colorPrimary};
      text-decoration: underline;
      cursor: pointer;
      margin-right: 4px;
      &:hover {
        color: ${token.colorPrimaryHover};
      }
    `,
  };
});

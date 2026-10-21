import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    appBar: css`
      flex-shrink: 0;
      display: flex;
      justify-content: space-between;
      align-items: center;
      height: 30px;
      background-color: ${token.colorBgLayout};
      border-bottom: 1px solid ${token.colorBorderLayout};
      user-select: none;
      -webkit-app-region: drag;
      z-index: 10001;
    `,
    windowsAppBar: css`
      height: 34px;
    `,
    logoContainer: css`
      display: flex;
      justify-content: center;
      align-items: center;
      padding-left: 12px;
      flex:1;
    `,
    appName: css`
      font-weight: bold;
      text-align: center;
      -webkit-app-region: no-drag;
    `,
    dropdown: css`
      -webkit-app-region: no-drag;
    `,
    logoRightSolt: css`
      display: flex;
      align-items: center;
    `,

    windowsActionBar: css`
      display: flex;
      -webkit-app-region: no-drag;
    `,
    windowsAction: css`
      width: 34px !important;
      height: 34px !important;
      border-radius: 0px !important;
      display: flex;
      justify-content: center;
      align-items: center;
      cursor: pointer;
      i{
        font-size: 14px;
      }
      &:hover {
        background-color: ${token.controlItemBgHover};
      }
    `,
    closeAction: css`
      &:hover {
        background-color: ${token.colorError};
        color: #fff;
      }
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, prefixCls }) => {
  return {
    wrapper: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      width: 220px;
      border-right: 1px solid ${token.colorBorderLayout};
      .${prefixCls}-menu {
        border-inline-end: none !important;
      }
    `,
    title: css`
      font-size: 16px;
      font-weight: 600;
      padding: 10px 16px;
      box-sizing: border-box;
      border-bottom: 1px solid ${token.colorBorderLayout};
    `,
    menuBox: css`
      flex: 1;
      height: 0px;
      overflow: auto;
      .ant-menu-light {
        background-color: ${token.colorBgBase} !important;
      }
    `,
    menuWrapper: css`
      color: ${token.colorTextSecondary};
    `,
  };
});

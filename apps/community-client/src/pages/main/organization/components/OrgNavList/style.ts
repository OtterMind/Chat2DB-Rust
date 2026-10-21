import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, prefixCls }) => {
  return {
    wrapper: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      .${prefixCls}-menu {
        border-inline-end: none !important;
      }
    `,

    menuBox: css`
      flex: 1;
      height: 0px;
      overflow: auto;
      .ant-menu-sub {
        background-color: transparent !important;
      }
    `,

    menuWrapper: css`
      color: ${token.colorTextSecondary};
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, prefixCls }) => {
  return {
    avatar: css`
      cursor: pointer;
      border: 1px solid ${token.colorFillQuaternary};
      /* background-color: ${token.colorFillTertiary}; */
      background-color: ${token.colorPrimaryBg};
      color: ${token.colorPrimary};
      width: 36px;
      height: 36px;
      &:hover {
        /* filter: brightness(1.1); */
        background-color: ${token.colorPrimaryBgHover};
      }
    `,
    dropdownBox: css`
      padding: 4px;
      background-color: ${token.colorBgBase};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadius}px;
      box-shadow: ${token.boxShadow};
      .ant-dropdown-menu {
        padding: 0;
        box-shadow: none;
        background-color: transparent;
        border: none;
        border-radius: 0;
      }
      .${prefixCls}-dropdown-menu-item-icon {
        font-size: 20px !important;
      }
      .${prefixCls}-dropdown-menu-item {
        margin: 2px 0 !important;
        padding: 6px 12px !important;
      }
      .${prefixCls}-dropdown-menu-item-divider {
        margin: 4px !important;
      }
      .${prefixCls}-dropdown-menu-submenu-selected .${prefixCls}-dropdown-menu-submenu-title {
        color: ${token.colorTextBase} !important;
      }
    `,

    userInfo: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      width: 240px;
      margin: 24px 0 16px 0;
    `,
    userName: css`
      display: flex;
      align-items: center;
      gap: 4px;
      margin-top: 16px;
      margin-bottom: 8px;
    `,
    displayName: css`
      font-weight: ${token.fontWeightStrong};
      line-height: 24px;

      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    displayOrg: css`
      font-weight: ${token.fontWeightStrong};
      line-height: 24px;
      margin-bottom: 8px;
      padding: 0px 12px;
      text-align: center;
    `,
    foreverIcon: css`
      font-size: ${token.fontSizeLG}px;
      color: #d48e21;
    `,
    menuSlot: css`
      margin-left: 6px;
      background-color: ${token.colorPrimary};
      font-size: 10px;
      line-height: 10px;
      color: ${token.colorPrimaryBg};
      padding: 2px 3px;
      border-radius: 3px;
    `,

    orgItem: css`
      display: inline-flex;
      align-items: center;
      justify-content: start;
      cursor: pointer;
      min-width: 160px;
      /* margin: 4px 6px; */
      padding: 4px 6px;
      border-radius: 4px;
      & svg {
        color: ${token.colorSuccess};
      }

      /* .${prefixCls}-dropdown-active {
        background-color: ${token.colorFillQuaternary};
      } */
    `,

    // orgItemChecked: css`
    //   background-color: ${token.colorFillTertiary};
    //   &:hover{
    //     background-color: transparent;
    //   }
    // `,
    orgItemLeft: css`
      display: flex;
      align-items: center;
      flex: 0 0 auto;
      margin-right: 60px;
      min-width: 160px;
      flex: 0 0 auto;
    `,
    orgItemName: css`
      margin-left: 12px;
      margin-right: 6px;
    `,
    orgItemTag: css`
      font-size: 10px;
      font-weight: ${token.fontWeightStrong};
      color: ${token.colorTextSecondary};
      border: 1px solid ${token.colorTextSecondary};
      border-radius: 2px;
      line-height: 10px;
      padding: 1px 2px;
    `,
  };
});

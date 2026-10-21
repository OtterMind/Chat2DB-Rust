import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    dropdownOverlay: css`
      .ant-dropdown-menu-submenu-title {
        display: flex;
        align-items: center;
      }
    `,
  };
});

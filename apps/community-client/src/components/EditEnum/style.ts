import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      display: inline-block;
    `,
    dropdownContent: css`
      width: 400px;
      height: 500px;
      border: 1px solid ${token.colorBorderSecondary};
      background: ${token.colorBgElevated};
      border-radius: 10px;
      
      .ant-dropdown-menu {
        max-height: 300px;
        overflow-y: auto;
      }
    `,
  };
});

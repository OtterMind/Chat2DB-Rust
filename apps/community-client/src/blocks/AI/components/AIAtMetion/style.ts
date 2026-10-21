import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      /* position: relative; */

      /* width: 100%; */
      max-height: 200px;
      overflow-y: auto;

      .ant-cascader-dropdown {
        min-width: 280px !important;
      }

      .ant-cascader-menu {
        min-width: 280px !important;
        height: auto !important;
      }

      .ant-cascader-menu-item {
        padding: 4px 8px !important;
        font-size: 12px;
      }
    `,
    content: css``,
    optionTitle: css`
      color: ${token.colorText};
      font-weight: bold;
      /* font-size: 12px; */
    `,
    optionExtra: css`
      color: ${token.colorTextDescription};
      font-size: 11px;
    `,
  };
});

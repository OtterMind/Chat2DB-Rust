import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      width: fit-content;
      max-width: 100%;
      height: 20px !important;
      & .ant-select-selector {
        font-size: 12px !important;
        padding: 0 !important;
      }
      .ant-select-clear,
      .ant-select-arrow {
        transform: translateX(6px);
      }
    `,
    popupContainer: css`
      .ant-cascader-menu {
        height: 200px !important;
        max-height: 200px;
      }

      .ant-cascader-menu-item {
        padding: 4px 8px !important;
        font-size: 12px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }
    `,
    dropdownRender: css`
      display: flex;
      align-items: center;
      gap: 4px;
    `,
    displayRender: css`
      display: flex;
      align-items: center;
      gap: 4px;
    `,
    displayRenderPlus: css`
      display: flex;
      align-items: center;
      justify-content: center;
      color: ${token.colorTextSecondary};
      cursor: pointer;

      &:hover {
        color: ${token.colorPrimary};
      }
    `,
    dropdownRenderIcon: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    dropdownRenderTitle: css`
      flex: 1;
      color: ${token.colorText};
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
    displayRenderPlaceholder: css`
      color: ${token.colorTextPlaceholder};
    `,
  };
});

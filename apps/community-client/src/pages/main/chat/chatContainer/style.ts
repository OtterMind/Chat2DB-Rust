import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    chatContainerBox: css`
      height: 100%;
      background-color: ${token.colorBgLayout};
      display: flex;
      flex-direction: column;
    `,
    emptyBox: css`
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    chatInputContainer: css`
      flex-shrink: 0;
      box-sizing: border-box;
      width: 100%;
      display: flex;
      justify-content: center;
      flex-direction: column;
      align-items: center;
      padding: 0px 16px;
      flex-shrink: 0;
      margin-bottom: 12px;
    `,
    aiCommonThink: css`
      color: ${token.colorTextSecondary};
      font-size: 12px;
      padding: 8px 60px;
      line-height: 16px;
    `,
    innerInput: css`
      width: 100%;
      max-width: 860px;
      box-sizing: border-box;
      textarea {
        line-height: 22px !important;
      }
    `,
    excelNameBox: css`
      max-width: 300px;
      display: flex;
      align-items: center;
      gap: 2px;
    `,
    excelNameIcon: css`
      color: ${token.colorTextSecondary};
      flex-shrink: 0;
    `,
    excelName: css`
      flex: 1;
      color: ${token.colorTextSecondary};
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
    bottomAddons: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
    `,
    modelSelect: css`
      opacity: 0.5;
      & .ant-select-selector {
        padding-left: 0px !important;
        font-size: 12px !important;
      }
    `,
  };
});

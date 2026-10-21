import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    HoverHelpContent: css`
      background-color: ${token.colorBgBase};
      padding: 12px;
      border-radius: 8px;
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
      z-index: 1000;
      max-width: 50vw;
      max-height: 60vh;
      overflow: hidden;
      overflow-y: auto;
      line-height: 1.2;
    `,
    HoverHelpTitle: css`
      font-size: 14px;
      font-weight: ${token.fontWeightStrong};
      color: ${token.colorText};
    `,
    HoverHelpValue: css`
      font-size: 14px;
      color: ${token.colorText};
    `,
  };
});

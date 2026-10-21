import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      background: linear-gradient(180deg, ${token.colorPrimaryBorder} 0%, ${token.colorBgBase} 40.74%);
      height: auto;
      width: 100%;
      display: flex;
      flex-direction: column;
      align-items: center;
      padding: 40px 0;
    `,
    wrapperExpired: css`
      background: linear-gradient(180deg, ${token.colorBorderSecondary} 0%, ${token.colorBgContainer} 100%);
    `,
    title: css`
      font-size: 20px;
      line-height: 24px;
      font-weight: ${token.fontWeightStrong};
      margin-top: 20px;
    `,

    highlightSubTitle: css`
      color: ${token.colorPrimary};
    `,
    divideline: css`
      margin-top: 20px;
      margin-bottom: 20px;
      font-size: 12px;
      font-style: normal;
      line-height: 14px;
      color: ${token.colorTextSecondary};
    `,
    leftDivideLine: css`
      width: 60px;
      height: 1px;
      background: linear-gradient(270deg, ${token.colorBorder} 0%, ${token.colorBgBase} 100%);
    `,
    rightDivideLine: css`
      width: 60px;
      height: 1px;
      background: linear-gradient(270deg, ${token.colorBgBase} 0%, ${token.colorBorder} 100%);
    `,

    featureIcon: css`
      border-radius: 8px;
      border: 1px solid ${token.colorPrimaryBorderHover};
      background: ${token.colorPrimaryBg};
      width: 42px;
      height: 42px;
    `,
    svgIcon: css`
      font-size: 28px;
      color: ${token.colorPrimary};
    `,

    featureTitle: css`
      color: ${token.colorText};
      font-size: 14px;
      font-weight: 500;
      line-height: 16px;
    `,
    featureSubTitle: css`
      font-size: 12px;
      font-weight: 400;
      line-height: 14px;
      color: ${token.colorTextSecondary};
    `,

    primaryButton: css`
      margin-top: 36px;
      /* margin-bottom: 8px; */
    `,
    secondaryButton: css`
      color: ${token.colorTextTertiary};
      font-size: ${token.fontSizeSM}px;
      line-height: ${token.lineHeightSM};
      &:hover {
        color: ${token.colorPrimary};
      }

      display: flex;
      align-items: center;
    `,
    chevronRight: css`
    `,
  };
});

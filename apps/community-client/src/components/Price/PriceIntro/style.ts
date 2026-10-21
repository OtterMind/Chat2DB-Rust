import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      padding: 0px 12px 32px 26px;
      display: flex;
      flex-direction: column;
      align-items: center;
      border-right: 1px solid ${token.colorBorderSecondary};
      width: 280px;
    `,
    divideline: css`
      display: flex;
      align-items: center;
      gap: 12px;
      margin: 24px 0;
      font-size: 12px;
      color: ${token.colorTextSecondary};

      &::before,
      &::after {
        content: '';
        flex-grow: 0;
        width: 24px;
        height: 1px;
      }

      &::before {
        background: linear-gradient(270deg, ${token.colorBorder} 0%, ${token.colorBgBase} 100%);
      }

      &::after {
        background: linear-gradient(270deg, ${token.colorBgBase} 0%, ${token.colorBorder} 100%);
      }
    `,

    featureIcon: css`
      border-radius: 4px;
      border: 1px solid ${token.colorPrimaryBorderHover};
      background: ${token.colorPrimaryBg};
      width: 32px;
      height: 32px;
      box-sizing: border-box;
    `,
    svgIcon: css`
      color: ${token.colorPrimary};
    `,
    featureTitle: css`
      font-size: 14px;
      line-height: 20px;
      flex: 1;
    `,
  };
});

import { createStyles, keyframes } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  const bounceArrow = keyframes`
    0%   { transform: translateX(0); }
    8%   { transform: translateX(5px); }
    16%  { transform: translateX(0); }
    22%  { transform: translateX(3px); }
    28%  { transform: translateX(0); }
    32%  { transform: translateX(1.5px); }
    36%  { transform: translateX(0); }
    100% { transform: translateX(0); }
  `;
  return {
    box: css`
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      width: 100%;
    `,
    letterpress: css`
      display: flex;
      justify-content: center;
      align-items: center;
      font-size: 80px;
      font-weight: 900;
      color: ${token.colorTextQuaternary};
      overflow: hidden;
      margin-bottom: 30px;
    `,
    shortcuts: css`
      margin-bottom: 32px;
    `,
    shortcutsItem: css`
      display: flex;
      justify-content: center;
      font-size: 14px;
      margin: 10px 0px;
    `,
    title: css`
      width: 200px;
      text-align: right;
      margin-right: 10px;
    `,
    plusSignBox: css`
      width: 200px;
      color: ${token.colorPrimary};
    `,
    plusSign: css`
      margin: 0px 4px;
    `,

    // AI introduction page styles.
    aiIntro: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 20px;
      text-align: center;
    `,

    aiIconWrap: css`
      width: 72px;
      height: 72px;
      border-radius: 18px;
      background: ${token.colorPrimaryBg};
      display: flex;
      align-items: center;
      justify-content: center;
    `,

    aiSpark: css`
      color: ${token.colorPrimary};
      font-size: 32px;
      line-height: 1;
    `,

    aiTitle: css`
      font-size: 32px;
      font-weight: 700;
      color: ${token.colorText};
    `,

    aiDesc: css`
      font-size: 16px;
      line-height: 1.7;
      color: ${token.colorTextSecondary};
      white-space: pre-line;
    `,

    featureRow: css`
      display: flex;
      gap: 12px;
    `,

    featureCard: css`
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 16px;
      border-radius: 8px;
      background: ${token.colorFillQuaternary};
      font-size: 14px;
      color: ${token.colorTextSecondary};
      cursor: default;
      user-select: none;
    `,

    featureIcon: css`
      color: ${token.colorTextTertiary};
      flex-shrink: 0;
    `,

    aiCta: css`
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 0 28px;
      height: 36px;
      font-size: 14px;
    `,

    aiCtaArrow: css`
      display: inline-flex;
      animation: ${bounceArrow} 3s ease-out infinite;
    `,

    dismissBtn: css`
      background: none;
      border: none;
      cursor: pointer;
      font-size: 12px;
      color: ${token.colorTextQuaternary};
      padding: 4px 8px;
      border-radius: 4px;
      transition: color 0.15s;

      &:hover {
        color: ${token.colorTextSecondary};
      }
    `,
  };
});

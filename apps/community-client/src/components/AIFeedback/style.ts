import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, cx, token }) => {
  const gradual = css`
    border: 1px solid transparent;
    background: none;
    background-clip: padding-box, border-box;
    background-origin: padding-box, border-box;
    background-image: linear-gradient(to right, ${token.colorBgBase}, ${token.colorBgBase}),
      radial-gradient(
        circle at 0 5%,
        rgba(255, 116, 236, 0.7),
        rgba(133, 112, 255, 0.7) 20%,
        rgba(133, 112, 255, 0.7) 30%,
        rgba(90, 239, 255, 0.7) 60%,
        rgba(90, 239, 255, 0.7) 80%,
        rgba(46, 150, 255, 1) 100%
      );
  `;
  return {
    container: css`
      /* padding-bottom: 16px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      margin-bottom: 8px; */
    `,
    tips: css`
      font-size: 14px;
      color: ${token.colorTextTertiary};
    `,
    acceptButton: cx(gradual && css``),
  };
});

import { createStyles, css, keyframes } from 'antd-style';

export const useStyles = createStyles(({ token }) => {
  const baseGradient = `linear-gradient(to right, ${token.colorBgBase}, ${token.colorBgBase})`;
  const gradientStops = [
    'rgba(115,90,255,0.5) -12deg',
    'rgba(0,127,255,0.5) 44deg',
    'rgba(90,239,255,0.5) 148deg',
    'rgba(211,153,255,0.5) 192deg',
    'rgba(255,133,56,0.5) 255deg',
    'rgba(255,116,236,0.5) 316deg',
    'rgba(115,90,255,0.5) 348deg',
    'rgba(0,127,255,0.5) 404deg',
  ].join(',');
  const moveGradient = keyframes`
  0% {
    background-image: ${baseGradient}, conic-gradient(from 0deg, ${gradientStops});
  }
  5% {
    background-image: ${baseGradient}, conic-gradient(from 18deg, ${gradientStops});
  }
  10% {
    background-image: ${baseGradient}, conic-gradient(from 36deg, ${gradientStops});
  }
  15% {
    background-image: ${baseGradient}, conic-gradient(from 54deg, ${gradientStops});
  }
  20% {
    background-image: ${baseGradient}, conic-gradient(from 72deg, ${gradientStops});
  }
  25% {
    background-image: ${baseGradient}, conic-gradient(from 90deg, ${gradientStops});
  }
  30% {
    background-image: ${baseGradient}, conic-gradient(from 108deg, ${gradientStops});
  }
  35% {
    background-image: ${baseGradient}, conic-gradient(from 126deg, ${gradientStops});
  }
  40% {
    background-image: ${baseGradient}, conic-gradient(from 144deg, ${gradientStops});
  }
  45% {
    background-image: ${baseGradient}, conic-gradient(from 162deg, ${gradientStops});
  }
  50%{
    background-image: ${baseGradient}, conic-gradient(from 180deg, ${gradientStops});
  }
  55% {
    background-image: ${baseGradient}, conic-gradient(from 198deg, ${gradientStops});
  }
  60% {
    background-image: ${baseGradient}, conic-gradient(from 216deg, ${gradientStops});
  }
  65% {
    background-image: ${baseGradient}, conic-gradient(from 234deg, ${gradientStops});
  }
  70% {
    background-image: ${baseGradient}, conic-gradient(from 252deg, ${gradientStops});
  }
  75% {
    background-image: ${baseGradient}, conic-gradient(from 270deg, ${gradientStops});
  }
  80% {
    background-image: ${baseGradient}, conic-gradient(from 288deg, ${gradientStops});
  }
  85% {
    background-image: ${baseGradient}, conic-gradient(from 306deg, ${gradientStops});
  }
  90% {
    background-image: ${baseGradient}, conic-gradient(from 324deg, ${gradientStops});
  }
  95% {
    background-image: ${baseGradient}, conic-gradient(from 342deg, ${gradientStops});
  }
  100%{
    background-image: ${baseGradient}, conic-gradient(from 360deg, ${gradientStops});
  }
`;
  return {
    aiMoveGradient: css`
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
      animation: 3s linear 0s infinite normal none running ${moveGradient};
      &:focus,
      &:hover {
        border: 1px solid transparent;
      }
    `,
  };
});

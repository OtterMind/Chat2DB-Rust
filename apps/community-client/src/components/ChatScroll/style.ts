import { createStyles, keyframes } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
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
  const buildGradient = (angle: number) =>
    [
      `linear-gradient(to right, ${token.colorBgBase}, ${token.colorBgBase})`,
      `conic-gradient(from ${angle}deg,${gradientStops})`,
    ].join(',');
  const moveGradient = keyframes`
    0% {
      background-image: ${buildGradient(0)};
    }
    5% {
      background-image: ${buildGradient(18)};
    }
    10% {
      background-image: ${buildGradient(36)};
    }
    15% {
      background-image: ${buildGradient(54)};
    }
    20% {
      background-image: ${buildGradient(72)};
    }
    25% {
      background-image: ${buildGradient(90)};
    }
    30% {
      background-image: ${buildGradient(108)};
    }
    35% {
      background-image: ${buildGradient(126)};
    }
    40% {
      background-image: ${buildGradient(144)};
    }
    45% {
      background-image: ${buildGradient(162)};
    }
    50%{
      background-image: ${buildGradient(180)};
    }
    55% {
      background-image: ${buildGradient(198)};
    }
    60% {
      background-image: ${buildGradient(216)};
    }
    65% {
      background-image: ${buildGradient(234)};
    }
    70% {
      background-image: ${buildGradient(252)};
    }
    75% {
      background-image: ${buildGradient(270)};
    }
    80% {
      background-image: ${buildGradient(288)};
    }
    85% {
      background-image: ${buildGradient(306)};
    }
    90% {
      background-image: ${buildGradient(324)};
    }
    95% {
      background-image: ${buildGradient(342)};
    }
    100%{
      background-image: ${buildGradient(360)};
    }
  `;
  return {
    container: css`
      overflow-y: auto;
      scroll-behavior: smooth;
      /* &::after {
        content: '';
        display: block;
        position: sticky;
        bottom: 0;
        left: 0;
        right: 0;
        height: 20px;
        background: linear-gradient(to bottom, transparent, ${token.colorBgBase});
        pointer-events: none;
        z-index: 1;
      } */
    `,
    scrollButton: css`
      position: absolute;
      bottom: 20px;
      left: 50%;
      margin-left: -20px;
      z-index: 1;
      border-radius: 50% !important;
      background-color: ${token.colorBgBase};
      border: 1px solid ${token.colorBorder};
      box-shadow: ${token.boxShadowTertiary};
      color: ${token.colorPrimary};
      &:hover {
        background-color: ${token.colorBgBase};
        color: ${token.colorPrimaryHover};
      }
    `,
    scrollButtonOutput: css`
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
      animation: 2s ease-in-out 0s infinite normal none running ${moveGradient};
      &:focus,
      &:hover {
        border: 1px solid transparent;
      }
    `,
    filler: css`
      height: 20px;
    `,
  };
});

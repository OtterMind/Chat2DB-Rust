import { createStyles, keyframes } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  const spinnerFade = keyframes`
    0% {
      background-color: ${token.colorText};
    }
    100% {
      background-color: transparent;
    }
  `;
  return {
    spinner: css`
      font-size: 14px;
      position: relative;
      display: inline-block;
      width: 1em;
      height: 1em;
    `,
    spinnerBlade: css`
      position: absolute;
      left: 0.4629em;
      bottom: 0;
      width: 0.074em;
      height: 0.2777em;
      border-radius: 0.0555em;
      transform-origin: center -0.2222em;
      animation: ${spinnerFade} 1s infinite linear;
      &:nth-child(1) {
        animation-delay: 0s;
        transform: rotate(0deg);
        background-color: ${token.colorText}08;
      }
      &:nth-child(2) {
        animation-delay: 0.083s;
        transform: rotate(30deg);
        background-color: ${token.colorText}16;
      }
      &:nth-child(3) {
        animation-delay: 0.166s;
        transform: rotate(60deg);
        background-color: ${token.colorText}24;
      }
      &:nth-child(4) {
        animation-delay: 0.249s;
        transform: rotate(90deg);
        background-color: ${token.colorText}32;
      }
      &:nth-child(5) {
        animation-delay: 0.332s;
        transform: rotate(120deg);
        background-color: ${token.colorText}40;
      }
      &:nth-child(6) {
        animation-delay: 0.415s;
        transform: rotate(150deg);
        background-color: ${token.colorText}48;
      }
      &:nth-child(7) {
        animation-delay: 0.498s;
        transform: rotate(180deg);
        background-color: ${token.colorText}56;
      }
      &:nth-child(8) {
        animation-delay: 0.581s;
        transform: rotate(210deg);
        background-color: ${token.colorText}64;
      }
      &:nth-child(9) {
        animation-delay: 0.664s;
        transform: rotate(240deg);
        background-color: ${token.colorText}72;
      }
      &:nth-child(10) {
        animation-delay: 0.747s;
        transform: rotate(270deg);
        background-color: ${token.colorText}80;
      }
      &:nth-child(11) {
        animation-delay: 0.83s;
        transform: rotate(300deg);
        background-color: ${token.colorText}90;
      }
      &:nth-child(12) {
        animation-delay: 0.913s;
        transform: rotate(330deg);
        background-color: ${token.colorText};
      }
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, { iconBoxSize }: { iconBoxSize: number; iconSize: number }) => {
  return {
    iconBox: css`
      height: ${iconBoxSize}px;
      width: ${iconBoxSize}px;
      display: flex;
      align-items: center;
      justify-content: center;
      border-radius: 4px;
      cursor: pointer;
      &:hover {
        background-color: ${token.colorFillSecondary};
      }
    `,
    activeIconBox: css`
      background-color: ${token.colorFillSecondary};;
    `,
    iconfont: css`
      font-family: 'iconfont' !important;
      font-size: var(--icon-size);
      font-style: normal;
      user-select: none;
      -webkit-font-smoothing: antialiased;
      -webkit-text-stroke-width: 0.2px;
      -moz-osx-font-smoothing: grayscale;
    `,
  };
});

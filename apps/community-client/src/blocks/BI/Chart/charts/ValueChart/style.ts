import { createStyles, css } from 'antd-style';

export const useStyles = createStyles(({ token }, { size, color }: { size: number; color?: string }) => {
  return {
    valueContainer: css`
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: ${size}px;
      color: ${color || token.colorPrimary};
    `,
  };
});

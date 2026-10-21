import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    dragBox: css``,
    draging: css`
      position: relative;
      &::before {
        position: absolute;
        content: '';
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        z-index: 1;
        background-color: ${token.colorFillTertiary};
        opacity: 0.5;
      }
    `
  };
});

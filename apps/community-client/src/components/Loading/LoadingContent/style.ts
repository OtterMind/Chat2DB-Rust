import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    loadingContent: css`
      position: relative;
    `,
    stateIndicator: css`
      width: 200px;
      height: 200px;
    `,
    empty: css`
      height: 100%;
      display: flex;
      justify-content: center;
      align-items: center;
      img {
        max-width: 100%;
      }
    `,
    coverLoading: css`
      position: absolute;
      inset: 0;
      background-color: rgba(0, 0, 0, 0.01);
      display: flex;
      justify-content: center;
      align-items: center;
      z-index: 999;
    `,
  };
});

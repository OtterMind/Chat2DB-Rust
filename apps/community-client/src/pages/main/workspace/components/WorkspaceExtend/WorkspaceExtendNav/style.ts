import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    workspaceExtendNav: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      width: 38px;
      padding: 8px 0px;
    `,
    topBox: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
      flex: 1;
    `,
    bottomBox: css`
      flex-shrink: 0;
    `,
    aiButton: css`
      width: 32px;
      height: 32px;
      padding: 0;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    aiIconWrapper: css`
      position: relative;
      width: 32px;
      height: 32px;
    `,
    defaultImg: css`
      position: absolute;
      width: 100%;
      height: 100%;
      transition: opacity 0.3s;
    `,
    hoverImg: css`
      position: absolute;
      width: 100%;
      height: 100%;
      opacity: 0;
      transition: opacity 0.3s;

      button:hover & {
        opacity: 1;
      }
    `,
  };
});

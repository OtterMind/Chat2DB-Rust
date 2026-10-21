import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    tableEditor: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    header: css`
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin: 10px 10px 0px 10px;
      box-sizing: border-box;
      overflow: hidden;
    `,
    headerLeft: css`
      display: flex;
      align-items: center;
      gap: 10px;
    `,
    main: css`
      flex: 1;
      position: relative;
      overflow: hidden;
    `,
    content: css`
      z-index: 2;
      position: absolute;
      left: 0px;
      top: 0px;
      width: 100%;
      height: 100%;
      opacity: 1;
    `,
    hiddenContent: css`
      z-index: 1;
      opacity: 0;
    `,
    sqlPreviewModalBox: css`
      position: relative;
    `,
  };
});

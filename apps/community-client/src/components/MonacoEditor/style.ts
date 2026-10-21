import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    editorContainer: css`
      height: 100%;
    `,
    editorContainerBox: css`
      position: relative;
      /* height: 500px; */
    `,
  };
});

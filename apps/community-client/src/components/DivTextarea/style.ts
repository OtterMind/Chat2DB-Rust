import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css`
      white-space: pre;
      overflow: auto;
      scrollbar-width: none; /* Hide the horizontal scrollbar in Firefox. */
      box-sizing: border-box;
      &:focus-visible {
        outline: none;
        cursor: text;
      }
    `,
  };
});

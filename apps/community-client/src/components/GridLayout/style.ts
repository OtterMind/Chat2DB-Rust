import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token },{gridBackground}:{gridBackground:string}) => {
  return {
    gridLayoutBox: css`
      min-height: 100%;
      transform: translate(0, 0);
      .dragHandle {
        cursor: grab;
      }
      .react-grid-item.react-grid-placeholder {
        background-color: ${token.colorPrimary};
        z-index: 0;
      }
    `,
    showDargBackground: css`
      background-size: calc(((100% - (13 * 8px)) / 12) + 8px) 98px; /* Adjust the background image size. */
      background-image: ${gridBackground};
      background-position: 8px 8px, 20px 0px;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      width: fit-content;
      border: 2px solid ${token.colorBorder};
      border-radius: 8px;
      background-color: ${token.colorBgBase};
      position: relative;
     
      .react-flow__handle {
        width: 10px;
        height: 10px;
        background-color: ${token.colorPrimary};
        border: 2px solid ${token.colorBorder};
        
        &:hover {
          background-color: ${token.colorPrimaryHover};
        }
      }
    `,
    tableHeader: css`
      display: flex;
      border-radius: 8px 8px 0 0;
      align-items: center;
      justify-content: center;
      background-color: ${token.colorPrimaryBg};
      line-height: 24px;
      padding: 4px 8px;
      /* Keep the table header and content border radii aligned. */
      border: 1px solid ${token.colorBorder};
      margin: -1px;
    `,
    tableContent: css`
      border-collapse: collapse;
      width: 100%;
    `,
    tableContentItem: css`
      position: relative;
      td {
        padding: 4px 8px;
        box-sizing: border-box;
        border-top: 1px solid ${token.colorBorder};
        max-width: 120px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    `,
    keyIcon: css`
    `,
    keyIconContent: css`
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    primaryKeyIcon: css`
      color: #EDAF13;
    `,
    // Customize the diamond style.
    diamond: css`
      width: 7px;
      height: 7px;
      border: 1px solid ${token.colorBorderSecondary};
      background-color: ${token.colorFillSecondary};
      transform: rotate(45deg);
    `,
    fieldHandle: css`
      width: 8px !important;
      height: 8px !important;
      border: 1px solid ${token.colorBorder} !important;
      opacity: 0;
      &:hover {
        background-color: ${token.colorPrimaryHover} !important;
      }
    `,
    fieldHandleActive: css`
      opacity: 1;
    `,
  };
});

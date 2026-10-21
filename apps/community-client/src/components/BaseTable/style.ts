import { createStyles } from 'antd-style';

export const useStyles = createStyles(
  ({ css, token }) => {
    return {
      plainText: css`
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        padding: 0 4px;
      `,
      tableCell: css`
        height: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      `,
      isSelectedTableCell: css`
        background-color: ${token.colorPrimaryBgHover};
      `,
      editTextTextClass: css`
        padding: 0px 4px;
        &:hover {
          border-radius: 0px;
        }
      `,
      editTextInputClass: css`
        border-radius: 0px !important;
      `,
      copyButton: css`
        display: inline-block;
        text-align: center;
        margin-left: 6px;
        transform: translateY(2px);
      `,
      spinBox: css`
        height: 100%;
        width: 100%;
        display: flex;
        justify-content: center;
        align-items: center;
      `,
      tooltipTitle: css`
        width: 100%;
        white-space: pre-wrap;
      `,
    };
  },
);

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    logBody: css`
      padding: 16px;
      border: 1px solid ${token.colorBorder};
      border-radius: 8px;
    `,
    logList: css`
      display: flex;
      flex-direction: column;
      margin-bottom: 16px;
      display: flex;
      flex-direction: column;
      gap: 8px;
    `,
    logListItem: css`
      display: flex;
      align-items: center;
      line-height: 14px;
    `,
    logListItemLabel: css`
      width: fit-content;
      flex-shrink: 0;
      text-align: right;
    `,
    logListItemValue: css`
      flex: 1;
      width: 0px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
  };
});

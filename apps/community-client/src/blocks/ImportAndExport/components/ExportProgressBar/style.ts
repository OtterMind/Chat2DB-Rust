import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, cx, prefixCls }) => {
  const taskItemHeader = cx(css`
    width: calc(100% - 22px);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    display: flex;
    align-items: center;
    justify-content: space-between;
  `)
  const taskName = cx(css`
    flex: 1;
    width: 0px;
    padding-right: 8px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  `)
  const taskTime = cx(css`
    flex-shrink: 0;
    font-size: 13px;
    color: ${token.colorTextSecondary};
  `)
  return {
    taskName,
    taskTime, 
    exportProgressBar: css`
      height: 24px;
      display: flex;
      align-items: center;
      font-size: 12px;
      padding: 0px 8px;
      border-top: 1px solid ${token.colorBorderLayout};
    `,
    left: css`
      width: 100px;
      font-size: ${token.colorTextSecondary};
      flex-shrink: 0;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    right: css`
      flex: 1;
      display: flex;
      align-items: center;
      gap: 6px;
    `,
    rightFirst: css`
      flex: 1;
      width: 0;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      text-align: right;
    `,
    progress: css`
      flex: 1;
      max-width: 200px;
    `,
    showAll: css`
      width: fit-content;
      flex-shrink: 0;
      padding: 0px 3px;
      user-select: none;
      &:hover {
        cursor: pointer;
        color: ${token.colorPrimary};
      }
    `,
    notification: css`
      .${prefixCls}-popover-inner {
        padding: 0;
      }
    `,
    wrapper: css`
      width: 280px;
    `,
    title: css`
      padding: 8px 10px;
      font-weight: ${token.fontWeightStrong};
      font-size: ${token.fontSizeLG}px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      display: flex;
      align-items: center;
      justify-content: space-between;
    `,
    listWrapper: css`
      padding: 0 8px;
      max-height: 400px;
      overflow-y: auto;
      overflow-x: hidden;
    `,
    listItem: css`
      padding: 6px 0px;
      cursor: pointer;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      &:last-child {
        border-bottom: none;
      }
    `,
    taskItemHeader,
    taskContent: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 8px;
    `,
    listItemLeft: css`
      width: 0px;
      flex: 1;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      font-size: 13px;
      color: ${token.colorTextSecondary};
    `,
    listItemRight: css`
      flex-shrink: 0;
      display: flex;
      gap: 4px;
      display: flex;
      align-items: center;
    `,
    taskItemProgress: css`
      /* line-height: 6px; */
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    statusBar: css`
      /* On Windows this area conflicts with the table scrollbar, so both selection and dragging are disabled. */
      user-select: none;
      -webkit-user-drag: none;
      position: relative;
      height: 26px;
      box-sizing: border-box;
      padding: 4px 8px;
      font-size: 12px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 16px;
      border-top: 1px solid ${token.colorBorderLayout};
      background-color: ${token.colorFillQuaternary};
      overflow: hidden;
      flex-shrink: 0;
      white-space: nowrap;
    `,
    resultSummary: css`
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;

      & > span {
        margin-right: 16px;
      }
    `,
    selectionSummary: css`
      display: flex;
      align-items: center;
      gap: 14px;
      flex-shrink: 0;
    `,
    metricButton: css`
      display: inline-flex;
      align-items: center;
      gap: 4px;
      height: 22px;
      margin: 0;
      padding: 0;
      color: ${token.colorTextSecondary};
      font: inherit;
      white-space: nowrap;
      cursor: pointer;
      background: transparent;
      border: 0;

      &:hover,
      &:focus-visible {
        color: ${token.colorText};
        outline: none;
      }
    `,
    metricLabel: css`
      color: inherit;
    `,
    metricValue: css`
      color: ${token.colorText};
      font-variant-numeric: tabular-nums;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    a: css``,
    editorChartSqlBox: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      gap: 2px;
      position: relative;
    `,
    backToChart: css``,
    draggablePanel: css`
    `,
  };
});

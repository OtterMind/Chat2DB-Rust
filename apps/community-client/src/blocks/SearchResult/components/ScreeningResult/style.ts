import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    screeningResult: css`
      display: flex;
      align-items: center;
      height: 26px;
      gap: 4px;
      flex-shrink: 0;
      .decorationsOverviewRuler{
        display: none !important;
      }
    `,
    whereBox: css`
      width: 50%;
      display: flex;
      height: 100%;
    `,
    orderByBox: css`
      width: 50%;
      display: flex;
      align-items: center;
      height: 100%;
    `,
    titleBox: css`
      display: flex;
      align-items: center;
      margin-right: 10px;
      width: fit-content;
      flex-shrink: 0;
    `,
    titleIcon: css`
      margin-right: 4px;
    `,
    title: css`
      flex-shrink: 0;
      font-weight: 400;
      height: 100%;
      line-height: 26px;
      color: ${token.colorPrimaryText};
    `,
    activeTitle: css`
    `,
    monacoEditor: css`
      flex: 1;
      height: 100%;
      width: 0px;
    `,
  };
});

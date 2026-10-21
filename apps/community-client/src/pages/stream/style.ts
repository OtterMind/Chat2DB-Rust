import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => ({
  fullContainer: css`
    width: 100%;
    height: 100%;
    overflow: hidden;
  `,

  paneInner: css`
    width: 100%;
    height: 100%;
    overflow: hidden;
  `,

  tableTabsContainer: css`
    width: 100%;
    height: 100%;
  `,
}));

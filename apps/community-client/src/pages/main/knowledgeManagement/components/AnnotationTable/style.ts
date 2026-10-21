import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    header: css`
      flex-shrink: 0;
    `,
    description: css`
      padding-bottom: 16px;
    `,
    treeContainer: css`
      flex: 1;
      height: 0px;
      display: flex;
    `,
    tree: css`
      width: 400px;
      border-right: 1px solid ${token.colorBorderSecondary};
    `,
    treeContent: css`
      flex: 1;
    `,
  };
});

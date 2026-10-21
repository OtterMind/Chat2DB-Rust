import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    tableTop: css`
      display: flex;
      justify-content: space-between;
      margin-bottom: 20px;
    `,
    modalTitle: css`
      font-size: 16px;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 8px;
    `,
    optIcon: css`
      cursor: pointer;
      font-size: 12px;

      &:hover {
        color: ${token.colorTextSecondary};
      }
    `,
    antdTable: css`
      flex: 1;
      height: 0px;
    `,
  };
});

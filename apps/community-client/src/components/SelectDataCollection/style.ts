import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    selectDataCollection: css`
      height: 26px;
      border-radius: 5px;
    `,
    select: css`
      width: 100%;
    `,
    toolbarBtnBox: css`
      display: flex;
      align-items: end;
    `,
    toolbarBtnBoxSecondLevel: css`
      height: 30px;
      display: flex;
      align-items: center;
      color: ${token.colorTextQuaternary};
    `,
    fullToolbarBtn: css`
      color: ${token.colorTextSecondary};
    `,
  };
});

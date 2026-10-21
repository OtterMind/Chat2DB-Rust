import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    notPermission: css`
      display: flex;
      flex-direction: column;
    `,
    notPermissionIconTips: css`
      color: ${token.colorTextTertiary};
      text-align: center;
      width: fit-content;
    `,
    notPermissionIcon: css`
      font-size: 200px;
      color: ${token.colorTextQuaternary};
      margin: 20px 0px;
    `,
    notPermissionIconBox: css`
      display: flex;
      justify-content: center;
    `,
  };
});

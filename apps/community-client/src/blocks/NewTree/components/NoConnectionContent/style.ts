import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    noConnectionBox: css`
      height: 100%;
    `,
    noConnectionContent: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      align-items: center;
      font-size: 14px;
      transform: translateY(25%);
    `,
    nodataSvg: css`
      margin-bottom: 22px;
    `,
    createButtonSvg: css`
      margin-right: 2px;
    `,
    noConnectionListTips: css`
      color: ${token.colorTextTertiary};
    `,
  };
});

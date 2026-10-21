import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      border-radius: 12px;
      border: 1px solid ${token.colorBorderSecondary};
      background-color: ${token.colorBgBase};
    `,
    header: css`
      display: flex;
      justify-content: space-between;
      padding: 12px;
      font-size: 12px;
      color: ${token.colorTextQuaternary};
      border-bottom: 1px solid ${token.colorBorderSecondary};
    `,
    content: css`
      padding: 20px 16px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    `,
    title: css`
      font-size: 16px;
      font-weight: ${token.fontWeightStrong};
    `,
    price: css`
      font-size: 16px;
      font-weight: ${token.fontWeightStrong};
    `,
    seat: css`
      color: ${token.colorTextSecondary};
      & span {
        color: ${token.colorPrimary};
      }
    `,
    timeDesc: css`
      font-size: 12px;
      color: ${token.colorTextQuaternary};
    `,
  };
});

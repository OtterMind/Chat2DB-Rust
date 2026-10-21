import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    planBox: css`
      /* border: 1px solid ${token.colorPrimaryBorder}; */
      background: ${token.colorFillQuaternary};
      border-radius: 8px;
      margin: 10px;
      padding: 10px 15px;
      width: 100%;
      box-sizing: border-box;
      margin: 20px auto;
    `,
    planHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      /* padding-bottom: 15px;
      border-bottom: 1px solid ${token.colorPrimaryBorder};
      margin-bottom: 15px; */
    `,
    planHeaderLeft: css`
      display: flex;
      align-items: center;
    `,
    verified: css`
      background-color: ${token.colorSuccessBgHover};
      color: ${token.colorSuccess};
    `,
    planHeaderRight: css`
      display: flex;
      align-items: center;
      gap: 8px;
    `,
    planTitle: css`
      font-size: 22px;
      font-weight: 500;
      margin-right: 10px;
    `,
    degreeOfUse: css`
      display: flex;
    `,
    degreeOfUseTitle: css`
      font-size: 18px;
      font-weight: 500;
    `,
    degreeOfUseValue: css`
      font-size: 22px;
      color: ${token.colorPrimary};
      margin-top: 10px;
    `,
    degreeOfUseContainer: css`
      flex: 1;
    `,
  };
});

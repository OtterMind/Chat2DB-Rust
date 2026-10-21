import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, { active, tag }: { active?: boolean; tag?: string }) => {
  const activePricingCard = css`
    border: 1px solid ${token.colorPrimary};
    background-color: ${token.colorPrimaryBg};
  `;
  return {
    pricingCard: css`
      cursor: pointer;
      overflow: hidden;
      min-width: 180px;
      box-sizing: border-box;
      border-radius: 12px;
      border: 1px solid ${token.colorBorderSecondary};
      padding: 0px 24px 24px 24px;
      background: ${token.colorBgBase};
      ${active ? activePricingCard : ''}
      &:hover {
        border: 1px solid ${token.colorPrimary};
      }
    `,
    tag: css`
      padding: 0 10px;
      margin-left: -28px;
      margin-bottom: 10px;
      background-color: ${token.colorErrorHover};
      color: ${token.colorWhite};
      display: inline-block;
      border-radius: 0 0 4px 0px;
      line-height: 22px;
    `,

    productName: css`
      margin-top: ${tag ? '0' : '32px'};
      font-size: 16px;
      line-height: 18px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 16px;
    `,
    productPrice: css`
      display: flex;
      align-items: baseline;
      font-size: 22px;
      font-weight: ${token.fontWeightStrong};
      line-height: 22px;
      color: ${token.colorPrimary};
      margin-bottom: 12px;
    `,
    teamUserUnit: css`
      font-size: 12px;
      line-height: 12px;
      margin-left: 2px;
      letter-spacing: 3px;
    `,
    description2: css`
      font-size: 14px;
      color: ${token.colorTextSecondary};
    `,
  };
});

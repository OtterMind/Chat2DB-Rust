import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    pricingModal: css`
      padding-bottom: 0;
      .ant-modal-close {
        top: 22px;
        right: 22px;
        border-radius: 999px;
        background: ${token.colorFillQuaternary};
      }
      .ant-modal-body {
        overflow: hidden;
        max-height: none;
        padding-block: 0 !important;
      }
    `,
    segmentedBox: css`
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      padding: 8px 0 28px;
      text-align: center;
      gap: 12px;
    `,
    segmentedTitle: css`
      font-size: clamp(34px, 5vw, 58px);
      line-height: 1;
      font-weight: 700;
      letter-spacing: -0.04em;
      color: ${token.colorText};
      font-family: ${token.fontFamily};
    `,
    segmentedSubTitle: css`
      max-width: 680px;
      font-size: 15px;
      line-height: 24px;
      color: ${token.colorTextSecondary};
    `,
    freeTrial: css`
      color: ${token.colorPrimary};
      font-weight: bold;
    `,
    segmentedList: css`
      display: flex;
      padding: 4px;
      background: ${token.colorFillQuaternary};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 999px;
      margin-top: 6px;
    `,
    segmentedItem: css`
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 8px;
      border-radius: 999px;
      min-width: 130px;
      line-height: 38px;
      font-weight: 600;
      border: 1px solid transparent;
      cursor: pointer;
      color: ${token.colorTextSecondary};
      padding: 0 16px;
    `,
    segmentedItemActive: css`
      background: ${token.colorBgContainer};
      border-color: ${token.colorPrimary};
      box-shadow: 0 0 0 1px ${token.colorPrimaryBg};
      color: ${token.colorText};
    `,
    segmentedBadge: css`
      padding: 4px 10px;
      border-radius: 999px;
      background: ${token.colorPrimaryBg};
      color: ${token.colorPrimary};
      font-size: 12px;
      font-weight: 700;
      line-height: 1;
      white-space: nowrap;
    `,
    pricingCardList: css`
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 14px;
      @media (max-width: 768px) {
        grid-template-columns: 1fr;
      }
    `,
    skeletonCard: css`
      border-radius: 24px;
      border: 1px solid ${token.colorBorderSecondary};
      padding: 18px 14px 18px;
      background: ${token.colorBgContainer};
      min-height: 480px;
    `,
    modalContent: css`
      background: ${token.colorBgContainer};
      padding: 24px 28px 30px;
      font-family: ${token.fontFamily};
    `,
  };
});

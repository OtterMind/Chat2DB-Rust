import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    payBlock: css`
      border-radius: 12px;
      padding: 18px 0px;
      /* border: 1px solid ${token.colorBorder}; */
      background: ${token.colorBgBase};
    `,
    payBlockTop: css`
      border-bottom: 1px dashed ${token.colorBorderSecondary};
      padding-bottom: 20px;
      margin-bottom: 20px;
    `,

    topSubTitle: css`
      font-size: 12px;
      color: ${token.colorTextTertiary};
    `,

    payBlockLeft: css`
      width: 140px;
      padding-right: 28px;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      .ant-segmented {
        border-radius: 4px;
      }
      .ant-segmented-item-label {
        min-height: 20px;
        height: 20px;
        line-height: 20px;
        padding: 0px 6px;
      }
    `,
    payQRCode: css`
      width: 104px;
      height: 104px;
      display: flex;
      align-items: center;
      justify-content: center;
      position: relative;
      border: 2px solid ${token.colorPrimary};
      border-radius: 4px;
      overflow: hidden;
      img {
        width: 100%;
        height: 100%;
      }
    `,
    payTypeBox: css`
      width: 22px;
      height: 22px;
      position: absolute;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      background: #fff;
      border-radius: 4px;
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    segmentedBox: css`
      margin-top: 8px;
      display: flex;
      justify-content: center;
    `,
    payBlockRight: css`
      padding: 9px 28px 0px 28px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    `,
    payBlockRightTitle: css`
      line-height: 14px;
      color: ${token.colorTextSecondary};
    `,
    pricePaidWrapper: css`
      display: flex;
      align-items: baseline;
      gap: 8px;
    `,
    priceActuallyPaid: css`
      font-size: 28px;
      line-height: 28px;
      font-weight: 600;
    `,
    priceOriginPaid: css`
      font-size: 20px;
      color: ${token.colorTextSecondary};
      text-decoration: line-through;
      margin-left: 6px;
    `,
    privacyAgreement: css`
      font-size: 12px;
      color: ${token.colorTextTertiary};
      /* a {
        color: ${token.colorTextTertiary};
        &:hover {
          color: ${token.colorPrimary};
        }
      } */
    `,
    inviteCode: css`
      display: flex;
      align-items: center;
      gap: 8px;
      font-size: 12px;
      & > span {
        color: ${token.colorTextTertiary};
      }
      & > input {
        width: 100px;
        height: 26px;
      }

      & > .error {
        color: ${token.colorError};
      }
      & svg {
        color: ${token.colorSuccess};
      }
    `,
    payButton: css`
      min-width: 260px;
      border-radius: 30px;
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 8px;
    `,
  };
});

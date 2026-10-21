import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: 12px;

      max-height: 80vh;
      overflow-y: auto;
      height: 100%;
    `,
    banner: css`
      width: 100%;
      height: 72px;
      border-radius: 6px;
      background-size: cover;
      cursor: pointer;
      &:hover {
        filter: brightness(1.1);
      }
    `,
    innerWrapper: css`
      padding: 28px 32px;
    `,
    tabs: css`
      .ant-tabs-nav-list {
        width: 99%;
      }
      .ant-tabs-tab {
        flex: 1;
        display: flex;
        justify-content: center;
        align-items: center;
        border: none !important;
        border-radius: 0 !important;
      }
    `,
    cardBlock: css`
      display: flex;
      flex-wrap: wrap;
      gap: 16px 12px;
      padding-bottom: 20px;
      border-bottom: 1px dashed ${token.colorBorderSecondary};
      min-height: 150px;
    `,
    pricingCard: css`
      flex: 1 1 30%;
    `,
    payBlock: css`
      border-radius: 12px;
      padding: 18px 25px;
      background: ${token.colorBgBase};
    `,
  };
});

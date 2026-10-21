import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    pricingModal: css`
      padding-bottom: 0px;
      .ant-modal-body{
        overflow: hidden;
        max-height: none;
        padding-block: 0px !important;
      }
    `,
  };
});

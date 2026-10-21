import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    unifiedConfirmationModal: css`
    
    `,
    checkContainer: css`
      margin: 15px 0px 25px;
      display: flex;
    `,
    inputConfirmContainer: css`
      margin-top: 16px;
    `,
    inputConfirmLabel: css`
      margin-bottom: 8px;
      color: ${token.colorTextSecondary};
      line-height: 20px;

      .chat2db-delete-confirm-target-name {
        color: ${token.colorText};
        font-weight: 600;
        border-bottom: 1px dashed ${token.colorTextSecondary};
      }
    `,
    inputConfirmMismatch: css`
      margin-top: 6px;
      color: ${token.colorError};
      line-height: 18px;
      font-size: 12px;
    `,
    modalTitle: css`
      display: flex;
      align-items: center;
      justify-content: center;
    `,
  };
});

import { createStyles } from 'antd-style';
// import { varStyles } from '@/styles/var';

export const useStyles = createStyles(({ css, token }) => {
  return {
    modal: css`
      position: relative;
      h3 {
        background-color: #fff;
        position: sticky;
        top: 0;
      }
      .ant-modal-header {
        padding: 8px 10px;
      }
      .ant-modal-title {
        width: calc(100% - 40px);
      }
      .ant-modal-body {
        padding: 0px 10px 10px;
      }
      .ant-modal-content {
        max-height: 80vh;
        display: flex;
        flex-direction: column;
        .ant-modal-header,
        .ant-modal-footer {
          flex-shrink: 0;
        }
        .ant-modal-body {
          flex: 1;
          height: 0;
          overflow-y: auto;
        }
      }
    `,
    message: css`
      display: flex;
      align-items: center;
      padding: 0px 8px;
    `,
    messageIcon: css`
      flex-shrink: 0;
      margin-right: 6px;
      font-size: 14px;
      color: ${token.colorErrorText};
    `,
    messageText: css`
      flex: 1;
      width: 0px;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    description: css`
      word-break: break-all;
      padding: 0px 10px;
      font-size: 13px !important;

      word-break: break-all;
      display: -webkit-box;
      -webkit-box-orient: vertical;
      box-orient: vertical;
      -webkit-line-clamp: 3;
      line-clamp: 3;
      overflow: hidden;
      text-overflow: ellipsis;
    `,
    notification: css`
      z-index: 999 !important;
      width: 300px !important;
      font-size: 12px !important;
      padding: 8px 4px 4px 4px !important;
      .ant-notification-notice-message {
        margin-bottom: 4px !important;
      }
      .ant-notification-notice-actions {
        margin-top: 4px !important;
      }
      .ant-notification-notice-close {
        top: 8px !important;
        inset-inline-end: 8px !important;
      }
    `,
    errorDetail: css`
      white-space: pre;
      overflow-x: auto;
    `,
    modalTitle: css`
      width: 100%;
      display: flex;
      align-items: center;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      i {
        font-size: 16px;
        font-weight: 400;
        color: ${token.colorErrorText};
        margin-right: 10px;
      }
    `,
    modalFooter: css`
      text-align: start;
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
    copyErrorTips: css`
      color: ${token.colorErrorText};
    `,
  };
});

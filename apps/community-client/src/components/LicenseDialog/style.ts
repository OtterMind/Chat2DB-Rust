import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ token, css }) => {
  return {
    wrapper: css`
      padding-top: 16px;
      padding-bottom: 8px;
      background: ${token.colorBgBase};
      display: flex;
      flex-direction: column;
      gap: 24px;
    `,
    title: css`
      font-size: 14px;
      font-weight: 700;
      color: ${token.colorTextHeading};
      display: flex;
      flex-direction: column;
      gap: 4px;
    `,
    titleDesc: css`
      font-size: 12px;
      color: ${token.colorTextTertiary};
    `,
    content: css``,
    tips: css`
      color: ${token.colorTextTertiary};
      font-size: 12px;
      & > div {
        line-height: 20px;
      }
      & .ant-btn {
        height: 20px;
        font-size: 12px;
        padding: 0;
      }
    `,
    bulletPoint: css`
      color: ${token.colorTextTertiary};
      margin-right: 4px;
      flex-shrink: 0;
    `,
    errorMessage: css`
      padding: 8px 0;
      color: ${token.colorError};
      font-size: 12px;
      & .ant-btn {
        color: ${token.colorError};
      }
      &:hover {
        cursor: pointer;
        text-decoration: underline;
      }
    `,
    warning: css`
      color: ${token.colorError};
    `,
    deviceId: css`
      flex: 1;
    `,
    link: css`
      color: ${token.colorPrimary};
      font-size: 12px;
      &:hover {
        color: ${token.colorPrimaryHover};
        cursor: pointer;
        text-decoration: underline;
      }
    `,
  };
});

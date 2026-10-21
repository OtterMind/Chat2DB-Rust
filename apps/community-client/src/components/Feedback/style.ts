import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      .ant-upload-list-item-action {
        &:hover {
          background: #373E47 !important;
        }
      }
    `,
    titleWrapper: css`
      padding: 24px 24px 16px 24px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      margin-bottom: 20px;
    `,
    title: css`
      font-size: ${token.fontSizeXL}px;
      font-weight: ${token.fontWeightStrong};
      display: flex;
      align-items: center;
      gap: 8px;
    `,

    contentWrapper: css`
      padding: 0 24px;
    `,

    description: css`
      padding-inline-start: 28px;
      color: ${token.colorTextTertiary};
      font-size: ${token.fontSizeSM}px;
    `,
    block: css`
      margin-bottom: 24px;
    `,
    segTitle: css`
      font-size: ${token.fontSize};
      font-weight: ${token.fontWeightStrong};
      line-height: 18px;
      margin-bottom: 20px;
    `,

    emoji: css`
      width: 56px;
      height: 56px;
      border-radius: 50%;
      border: 1px solid ${token.colorBorderSecondary};
      display: flex;
      justify-content: center;
      align-items: center;
      cursor: pointer;
      transition: scale 400ms ${token.motionEaseOut};
      &:hover {
        border: 1px solid ${token.colorBorder};
      }
      &:active {
        scale: 0.9;
      }
    `,
    emojiActive: css`
      border: 1px solid ${token.colorPrimary} !important;
    `,

    improveBlock: css`
      padding: 6px 8px;
      border-radius: 4px;
      border: 1px solid ${token.colorBorder};
      display: flex;
      align-items: center;
      gap: 2;
      transition: scale 400ms ${token.motionEaseOut};
      cursor: pointer;
      font-size: ${token.fontSizeSM}px;
      font-weight: 600;
      &:active {
        scale: 0.95;
      }
    `,
    improveBlockActive: css`
      color: ${token.colorPrimary};
      border: 1px solid ${token.colorPrimary};
    `,

    textArea: css`
      margin: 12px 0;
    `,

    imageTips: css`
      color: ${token.colorTextTertiary};
      font-size: ${token.fontSizeSM}px;
      padding: 4px;
      margin-bottom: 24px;
    `,

    checkbox: css`
      color: ${token.colorTextTertiary};
      font-size: ${token.fontSizeSM}px;
    `,
    footer: css`
      padding: 24px;
      display: flex;
      justify-content: flex-end;
      gap: 8px;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, { active }: { active?: boolean }) => {
  return {
    cardBox: css`
      min-height: 580px;
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      justify-content: flex-start;
      padding: 18px 14px 18px;
      border-radius: 24px;
      border: 1px solid ${token.colorBorderSecondary};
      background: ${token.colorBgContainer};
      flex: 1;
      transition:
        transform 0.2s ease,
        border-color 0.2s ease,
        box-shadow 0.2s ease;
      &:hover {
        transform: translateY(-2px);
        border-color: ${token.colorBorder};
      }
    `,
    cardBoxActive: css`
      border-color: ${token.colorPrimary};
      box-shadow: 0 12px 32px ${token.colorBgMask};
    `,
    ul: css`
      flex: 1;
      font-size: 15px;
      color: ${token.colorTextSecondary};
      display: flex;
      flex-direction: column;
      gap: 12px;
      margin: 14px 0 0;
      padding: 0;
      list-style: none;
    `,
    uiLi: css`
      position: relative;
      padding-left: 20px;
      line-height: 22px;
    `,
    check: css`
      color: ${token.colorPrimary};
      position: absolute;
      left: 0;
      top: 2px;
    `,
    headerContainer: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
      margin-bottom: 14px;
      min-height: 152px;
    `,
    firstLine: css`
      font-size: 14px;
      line-height: 18px;
      color: ${token.colorTextSecondary};
      text-transform: uppercase;
      letter-spacing: 0.08em;
      font-weight: 600;
    `,
    secondLine: css`
      font-size: 20px;
      line-height: 28px;
      font-weight: 700;
      min-height: 56px;
    `,
    thirdLine: css`
      display: flex;
      align-items: flex-end;
      gap: 8px;
      flex-wrap: wrap;
      min-height: 48px;
    `,
    thirdLine1: css`
      font-size: 48px;
      line-height: 1;
      font-weight: 700;
      color: ${token.colorText};
      font-family: ${token.fontFamily};
    `,
    thirdLine2: css`
      font-size: 18px;
      line-height: 28px;
      color: ${token.colorTextSecondary};
    `,
    thirdLine3: css`
      min-height: 20px;
      font-size: 14px;
      line-height: 20px;
      color: ${token.colorTextTertiary};
    `,
    payButton: active
      ? css`
          border-radius: 999px;
          height: 40px;
          font-size: 16px;
          font-weight: 700;
          border: none;
          background: ${token.colorPrimary};
          color: ${token.colorWhite} !important;
          &:hover {
            background: ${token.colorPrimaryHover} !important;
            color: ${token.colorWhite} !important;
          }
        `
      : css`
          border-radius: 999px;
          height: 40px;
          font-size: 16px;
          font-weight: 700;
          border: 1px solid ${token.colorBorderSecondary};
          background: ${token.colorFillQuaternary};
          color: ${token.colorText} !important;
          &:hover {
            background: ${token.colorPrimaryBg} !important;
            border-color: ${token.colorPrimary} !important;
            color: ${token.colorPrimary} !important;
          }
        `,
  };
});

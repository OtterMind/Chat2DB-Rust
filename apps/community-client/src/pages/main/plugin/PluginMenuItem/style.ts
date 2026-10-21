import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, { isActive }: { isActive: boolean }) => {
  return {
    container: css`
      display: flex;
      background-color: ${isActive ? token.colorBgElevated : token.colorBgBase};
      padding: 16px 20px;
      gap: 12px;
      cursor: pointer;
      &:hover {
      }
    `,
    icon: css`
      /* width: 50px; */
      /* height: 50px; */
      display: flex;
      justify-content: center;
      align-items: center;
      & img {
        width: 36px;
        height: 36px;
      }
    `,
    title: css`
      font-size: 14px;
      font-weight: ${token.fontWeightStrong};
      color: ${token.colorTextBase};
    `,
    description: css`
      font-size: 12px;
      color: ${token.colorTextTertiary};
      display: flex;
      justify-content: space-between;
    `,
    downloadButton: css`
      font-size: 12px;
      border-radius: 4px;
      padding: 4px 12px;
      color: ${token.colorBgElevated};
      background-color: ${token.colorPrimary};
      cursor: pointer;
      &:hover {
        opacity: 0.8;
      }
    `,
  };
});

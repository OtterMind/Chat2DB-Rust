import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      flex-shrink: 0;
      height: 36px;
      padding: 0px 10px;
      box-sizing: border-box;
      width: 100%;
      background-color: ${token.colorPrimaryBg};
      display: flex;
      justify-content: space-between;
      align-items: center;
      color: ${token.colorPrimary};
      cursor: pointer;
      overflow: hidden;
      &:hover {
        background-color: ${token.colorPrimaryBgHover};
      }
    `,
    teamWrapper: css`
      color: ${token.colorTextBase};
      background-color: ${token.colorFillQuaternary};
      &:hover {
        background-color: ${token.colorFillTertiary};
      }
      .ant-btn-primary {
        background-color: ${token.colorTextBase};
        &:hover {
          background-color: ${token.colorTextBase} !important;
        }
      }
    `,
    left: css`
      flex: 1;
      margin-right: 6px;
    `,
    icon: css`
      flex-shrink: 0;
      font-size: 16px;
    `,
    text: css`
      flex: 1;
      width: 0;
      font-weight: ${token.fontWeightStrong};
      line-height: 14px;
      font-size: 12px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    `,

    edition: css`
      flex-shrink: 0;
      font-size: 10px;
      line-height: 10px;
      font-style: normal;
      font-weight: 700;
      border-radius: 2px;
      border: 1px solid ${token.colorPrimaryActive};
      padding: 1px 3px;
    `,
    buttonRight: css`
      display: block;
      max-width: 80px;
      min-width: 50px;
      color: ${token.colorBgBase};
      &:hover {
        opacity: 0.8;
        color: ${token.colorBgBase} !important;
      }
    `,
  };
});

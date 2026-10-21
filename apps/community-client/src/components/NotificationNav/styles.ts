import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, prefixCls }) => {
  return {
    notification: css`
      /* .${prefixCls}-popover-inner {
        border: none !important;
      } */
      .${prefixCls}-popover-inner {
        padding: 0;
      }
    `,
    wrapper: css`
      width: 280px;
    `,

    navIcon: css`
      position: relative;
    `,
    navUnreadIcon: css`
      position: absolute;
      width: 4px;
      height: 4px;
      border-radius: 50%;
      background-color: red;
      position: absolute;
      right: 11px;
      top: 10px;
      border: 1px solid ${token.colorBgElevated};
    `,

    title: css`
      padding: 12px 16px;
      font-weight: ${token.fontWeightStrong};
      font-size: ${token.fontSizeLG}px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      display: flex;
      align-items: center;
      justify-content: space-between;
    `,

    listWrapper: css`
      padding: 0 8px;
      max-height: 200px;
      overflow-y: auto;
      overflow-x: hidden;
    `,

    item: css`
      padding: 12px 0;
      display: flex;
      align-items: center;
      gap: 12px;
      cursor: pointer;
      &:hover {
        background-color: ${token.colorBgTextHover};
      }
      &:not(:last-child) {
        border-bottom: 1px solid ${token.colorBorderSecondary};
      }
    `,
    itemInvalid: css`
      cursor: not-allowed;
      opacity: 0.5;
    `,
    itemIconWrapper: css`
      width: 32px;
      height: 32px;
      position: relative;
      margin-left: 8px;
    `,
    itemIcon: css`
      font-size: 32px;
    `,
    itemIconUnread: css`
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background-color: red;
      position: absolute;
      right: -2px;
      top: -2px;
      border: 1px solid ${token.colorBgElevated};
    `,
    itemTitle: css`
      font-size: ${token.fontSize}px;
      line-height: 14px;
    `,
    itemSubTitle: css`
      font-size: ${token.fontSizeSM}px;
      line-height: 14px;
      color: ${token.colorTextSecondary};
    `,

    sModal: css`
      .${prefixCls}-modal-content {
        overflow: unset !important;
        background-color: transparent;
        border: none;
        box-shadow: none;
        &::-webkit-scrollbar{
          display: none;
        }
      }
      .${prefixCls}-modal-close {
        top: -32px;
        right: -32px;
      }
      .${prefixCls}-modal-body {
        padding-block: 0 !important;
        padding-inline: 0 !important;
        overflow: hidden;
      }

      & img {
        width: 100%;
        height: 100%;
        object-fit: contain;
        cursor: pointer;
      }
    `,

    noSModal: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
      padding: 12px;
      & img{
        width: 158px;
        height: auto;
        border-radius: 8px;
      }
    `,

  };
});

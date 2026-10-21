import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      flex-direction: column;
      min-height: 0;
    `,
    fields: css`
      flex: 1;
      min-height: 0;
      overflow: auto;
    `,
    item: css`
      padding: 7px 10px;

      &:focus-within [data-row-detail-action='true'],
      &[data-active='true'] [data-row-detail-action='true'] {
        opacity: 1;
        pointer-events: auto;
      }
    `,
    field: css`
      min-width: 0;
      margin-bottom: 5px;
      overflow: hidden;
      color: ${token.colorTextSecondary};
      font-size: 12px;
      line-height: 18px;
      white-space: nowrap;
      text-overflow: ellipsis;
    `,
    valueRow: css`
      position: relative;
      min-width: 0;
    `,
    valueWrapper: css`
      min-width: 0;
    `,
    valueInput: css`
      width: 100%;
      height: 34px;
      padding: 6px 38px 6px 9px;
      color: ${token.colorText};
      font-family: ${token.fontFamilyCode};
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorder};
      border-radius: 4px;
      box-shadow: none;

      &:focus {
        border-color: ${token.colorPrimary};
        box-shadow: 0 0 0 1px ${token.colorPrimary};
      }
    `,
    activeValueInput: css`
      border-color: ${token.colorPrimary};
      box-shadow: 0 0 0 1px ${token.colorPrimary};
    `,
    action: css`
      position: absolute;
      top: 1px;
      right: 2px;
      display: flex;
      align-items: center;
      justify-content: center;
    `,
    actionButton: css`
      width: 32px;
      height: 32px;
      padding: 0;
      color: ${token.colorTextSecondary};
      opacity: 0;
      pointer-events: none;
      transition: opacity 120ms ease;

      &:hover {
        color: ${token.colorText} !important;
      }
    `,
    nullValue: css`
      color: ${token.colorTextTertiary};
      font-family: ${token.fontFamily};

      &::placeholder {
        color: ${token.colorTextTertiary};
      }
    `,
  };
});

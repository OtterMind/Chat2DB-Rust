import { createStyles, Theme } from 'antd-style';

export const useStyles = createStyles(({ css }, { theme }: { theme: Omit<Theme, "prefixCls"> | null }) => {
  return {
    input: css`
      height: 100%;
      border: none;
      border-radius: 0px;
      padding: 0px 5px;
      background-color: ${theme?.colorBgContainer};
      font-size: 13px;
      /* This does not take effect in the input */
      font-family: ${theme?.fontFamily};
      font-weight: 400;
      color: ${theme?.colorText};
      &:focus {
        background-color: ${theme?.colorBgContainer};
      }
      &:hover {
        background-color: ${theme?.colorBgContainer};
      }
    `,
    textArea: css`
      height: 100%;
      min-height: 26px !important;
      padding-top: 5px !important;
      line-height: 18px !important;
      /* Do not break if /n is not encountered */
      white-space: pre;
    `,
    datePicker: css`
      width: 100%;
      height: 100%;
      border-radius: 0;
      border-color: ${theme?.colorBorder};
      color: ${theme?.colorText};
      background: ${theme?.colorBgContainer} !important;
      box-shadow: none;
      &.ant-picker,
      &.ant-picker-outlined,
      &.ant-picker-focused,
      &.ant-picker:hover {
        border-color: ${theme?.colorBorder};
        background: ${theme?.colorBgContainer} !important;
        box-shadow: none;
      }
      .ant-picker-input > input {
        color: ${theme?.colorText};
        background: transparent;
        &::placeholder {
          color: ${theme?.colorTextTertiary};
        }
      }
      .ant-picker-suffix {
        color: ${theme?.colorTextSecondary};
      }
    `,
    datePickerPopup: css`
      z-index: 9999;
      .ant-picker-panel-container {
        color: ${theme?.colorText};
        background: ${theme?.colorBgElevated || theme?.colorBgContainer};
        border: 1px solid ${theme?.colorBorderSecondary};
        box-shadow: ${theme?.boxShadowSecondary || theme?.boxShadow};
      }
      .ant-picker-panel,
      .ant-picker-time-panel,
      .ant-picker-time-panel-column {
        color: ${theme?.colorText};
        background: ${theme?.colorBgElevated || theme?.colorBgContainer};
      }
      .ant-picker-header,
      .ant-picker-content th,
      .ant-picker-cell,
      .ant-picker-time-panel-cell {
        color: ${theme?.colorText};
      }
      .ant-picker-header button,
      .ant-picker-header-view button {
        color: ${theme?.colorText};
      }
      .ant-picker-cell-in-view {
        color: ${theme?.colorText};
      }
      .ant-picker-cell-disabled,
      .ant-picker-cell:not(.ant-picker-cell-in-view) {
        color: ${theme?.colorTextDisabled};
      }
      .ant-picker-time-panel-cell-inner {
        color: ${theme?.colorText} !important;
      }
      .ant-picker-cell-inner:hover,
      .ant-picker-cell-today .ant-picker-cell-inner,
      .ant-picker-time-panel-cell-inner:hover {
        background: ${theme?.colorFillTertiary} !important;
      }
      .ant-picker-cell-selected .ant-picker-cell-inner,
      .ant-picker-time-panel-cell.ant-picker-time-panel-cell-selected .ant-picker-time-panel-cell-inner,
      .ant-picker-time-panel-column > li.ant-picker-time-panel-cell-selected .ant-picker-time-panel-cell-inner {
        color: ${theme?.colorTextLightSolid} !important;
        background: ${theme?.colorPrimary} !important;
      }
      .ant-picker-footer {
        border-top-color: ${theme?.colorBorderSecondary};
      }
      .ant-picker-now-btn {
        color: ${theme?.colorPrimary};
      }
    `,
  };
});

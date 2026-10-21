import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
  shortcutInput: css`
    width: 200px;
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid ${token.colorBorder};
    border-radius: 6px;
    background-color: ${token.colorBgContainer};
    color: ${token.colorText};
    cursor: text;
    outline: none;

    &:hover {
      border-color: ${token.colorPrimaryHover};
    }

    &:disabled {
      background-color: ${token.colorBgContainer};
      color: ${token.colorTextDisabled};
      cursor: not-allowed;
    }
  `,
  shortcutInputFocused: css`
    border-color: ${token.colorPrimary};
    box-shadow: 0 0 0 2px ${token.colorPrimaryBg};
  `,
  shortcutKey: css`
    min-width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 6px;
    border: 1px solid ${token.colorBorderSecondary};
    border-radius: 5px;
    background-color: ${token.colorFillQuaternary};
    color: ${token.colorText};
    font-size: 13px;
    line-height: 18px;
    white-space: nowrap;
  `,
  placeholder: css`
    color: ${token.colorTextPlaceholder};
  `,
}));

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
  label: css`
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    width: 100%;
    min-width: 0;
  `,
  text: css`
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  `,
  shortcut: css`
    flex-shrink: 0;
    color: ${token.colorTextQuaternary};
    font-size: 12px;
  `,
}));

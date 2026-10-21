import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
  container: css`
    height: 100%;
    padding: 8px 10px;
    overflow: auto;
  `,
  item: css`
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr);
    gap: 8px;
    min-height: 34px;
    padding: 7px 4px;
    align-items: start;
    font-size: 12px;
    line-height: 20px;
  `,
  label: css`
    min-width: 0;
    overflow: hidden;
    color: ${token.colorText};
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  `,
  value: css`
    min-width: 0;
    overflow-wrap: anywhere;
  `,
  valueButton: css`
    display: flex;
    min-width: 0;
    align-items: flex-start;
    gap: 6px;
    padding: 0;
    color: ${token.colorText};
    font-family: ${token.fontFamilyCode};
    font-size: inherit;
    font-variant-numeric: tabular-nums;
    line-height: inherit;
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: 0;

    &:hover,
    &:focus-visible {
      color: ${token.colorPrimary};
      outline: none;
    }

    &:hover svg,
    &:focus-visible svg {
      opacity: 1;
    }
  `,
  copyIcon: css`
    flex-shrink: 0;
    margin-top: 3px;
    opacity: 0;
    transition: opacity 120ms ease;
  `,
}));

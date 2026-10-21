import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, { size }: { size: 'sm' | 'md' }) => {
  const smTitleFontSize = css`
    font-size: 14px;
  `
  const mdTitleFontSize = css`
    font-size: 18px;
    font-weight: 600;
  `
  return {
    modalTitle: css`
      display: flex;
      align-items: center;
    `,
    prefixIcon: css`
      margin-right: 8px;
    `,
    title: css`
      ${size === 'sm' ? smTitleFontSize : mdTitleFontSize}
    `,
    tipsIconContainer: css`
      display: flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      margin-left: 2px;
    `,
    tipsIcon: css`
      margin-left: 4px;
      color: ${token.colorTextTertiary};
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    cascader: css`
      height: 36px;
    `,
    optionItem: css`
      display: inline-flex;
      align-items: center;
      gap: 4px;
      height: 26px;
    `,
    optionItemIcon: css`
      color: ${token.colorPrimary};
    `,
  };
});

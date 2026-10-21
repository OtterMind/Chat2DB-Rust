import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, prefixCls }) => {
  return {
    wrapper: css`
      .${prefixCls}-modal-body {
        background: ${token.colorFillTertiary};
      }
    `,
    empty: css`
      margin-top: 40px;
      margin-bottom : 80px;
    `
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      height: 100%;
      display: flex;
      .ant-splitter-bar-dragger::before {
        background-color: ${token.colorBorderLayout} !important;
        width: 1px !important;
      }
    `,
  };
});

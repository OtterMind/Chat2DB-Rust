import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    input: css`
      width: 40px;
      height: 52px;
      text-align: center;
      font-weight: ${token.fontWeightStrong};
      font-size: 24px;
      background-color: ${token.colorFillTertiary};
      border: none;
      caret-color: transparent;
      &:active,
      &:hover {
        background-color: ${token.colorFillTertiary} !important;
      }
      &:focus {
        background-color: ${token.colorFillSecondary} !important;
      }
    `,
  };
});

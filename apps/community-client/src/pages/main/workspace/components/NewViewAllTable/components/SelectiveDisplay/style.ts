import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    a: css``,
    selectiveDisplay: css`
      flex-shrink: 0;
      height: 24px;
      display: flex;
      padding: 0px 8px 0px 8px;
      gap: 3px;
      align-items: center;
      background-color: ${token.colorFillTertiary};
      font-size: 12px;
    `,
    showAll: css`
      color: ${token.colorPrimary};
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimaryActive};
      }
    `,
    dropdownRender: css`
      min-width: 180px;
      display: flex;
      flex-direction: column;
      padding: 20px;
      border-radius: 16px;
      box-shadow: ${token.boxShadow};
      border: 1px solid ${token.colorBorderSecondary};
      background-color: ${token.colorBgBase};
    `,
    dropdownRenderTitle: css`
      margin-bottom: 18px;
    `,
    dropdownRenderBody: css`
      max-height: 200px;
      display: flex;
      flex-direction: column;
      gap: 8px;
      overflow-y: auto;
    `,
  };
});

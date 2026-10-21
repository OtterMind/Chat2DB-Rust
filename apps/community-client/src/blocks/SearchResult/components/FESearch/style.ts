import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      border-top: 1px solid ${token.colorBorderSecondary};
      height: 28px;
    `,
    resultSetSearchBar: css`
      flex: 1;
      border: 0px;
      background: none;
    `,
    count: css`
      flex-shrink: 0;
      font-size: 12px;
      line-height: 12px;
      transform: translateY(1px);
    `,
    noSearchResult: css`
      color: ${token.colorErrorText};
    `,
    buttonGroup: css`
      flex-shrink: 0;
      display: flex;
      align-items: center;
      margin-left: 8px;
      gap: 4px;
      padding-right: 10px;
    `,
  };
});

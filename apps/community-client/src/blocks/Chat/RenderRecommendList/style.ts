import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    renderRecommendList: css`
      width: 100%;
      padding-left: 4px;
    `,
    recommendList: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
      width: fit-content;
    `,
    recommendListItem: css`
      width: fit-content;
      padding: 6px 10px;
      border-radius: 4px;
      background-color: ${token.colorFillQuaternary};
      cursor: pointer;
      &:hover {
        filter: brightness(1.2);
      }
    `,
  };
});

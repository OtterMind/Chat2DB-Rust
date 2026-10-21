import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    chatQuestionTemplateBox: css`
      display: flex;
      justify-content: center;
      padding: 20px 20px;
    `,
    chatQuestionTemplate: css`
      display: flex;
      flex-wrap: wrap;
      justify-content: space-around;
      gap: 14px 10px;
      width: 95%;
    `,
    questionTemplateItem: css`
      padding: 8px 14px;
      border-radius: 10px;
      display: flex;
      flex-direction: column;
      gap: 2.5px;
      background-color: ${token.colorBgBase};
      width: calc(50% - 14px);
      box-sizing: border-box;
      cursor: pointer;
      transition: transform 1s, box-shadow 1s;
      &:hover {
        box-shadow: ${token.boxShadowTertiary};
        transform: translateY(-4px);
        transition: transform 1s, box-shadow 1s;
        /* background-color: ${token.colorFillSecondary}; */
      }
    `,
    title: css`
      font-size: 14px;
      font-weight: 500;
    `,
    description: css`
      color: ${token.colorTextSecondary};
      font-size: 13px;
      display: -webkit-box;
      -webkit-box-orient: vertical;
      -webkit-line-clamp: 2;
      text-overflow: ellipsis;
      overflow-y: auto;
    `,
    code: css`
      pre {
        padding-left: 0px !important;
        padding-bottom: 0px !important;
      }
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      flex-shrink: 0;
      padding: 4px 8px;
      border-bottom: 1px solid ${token.colorBorderLayout};
      box-sizing: border-box;
      height: 36px;
      display: flex;
      justify-content: space-between;
    `,
    titleContainer: css`
      flex: 1;
      width: 0px;
    `,
    extraBtnContainer: css`
      flex-shrink: 0;
    `,
    skeleton: css``,

    avatar: css`
      flex-shrink: 0;
      font-size: 18px;
      min-width: 24px;
    `,
    title: css`
      flex: 1;
      /* width: 0px; */
      /* max-width: 30%; */
      overflow: hidden;
      font-weight: bold;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 14px;
      opacity: 0.9;
    `,
  };
});

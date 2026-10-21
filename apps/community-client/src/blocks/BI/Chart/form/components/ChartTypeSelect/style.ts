import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    groupContainer: css`
      display: flex;
      flex-direction: column;
      gap: 14px;
      padding: 2px 10px;
    `,
    groupItem: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
    `,
    groupTitle: css`
      font-size: 14px;
      font-weight: 500;
    `,
    componentContainer: css`
      display: flex;
      gap: 10px;
      flex-wrap: wrap;
    `,
    componentItem: css`
      height: 60px;
      width: 80px;
      background-color: ${token.colorFillQuaternary};
      display: flex;
      justify-content: center;
      align-items: center;
      border-radius: 6px;
      cursor: pointer;
      border: 1px solid transparent;
      &:hover {
        background-color: ${token.colorFillTertiary};
        border: 1px solid ${token.colorBorder};
      }
    `,
    label: css`
      display: flex;
      gap: 4px;
      align-items: center;
    `,
  };
});

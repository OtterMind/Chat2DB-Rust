import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      display: flex;
      flex-direction: column;
      gap: 8px;
    `,
    addAxisButton: css`
      display: flex;
      align-items: center;
      gap: 4px;
      width: fit-content;
      padding: 4px;
      border-radius: 4px;
      color: ${token.colorPrimary};
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimaryHover};
      }
      &:active {
        color: ${token.colorPrimaryActive};
      }
    `,
    axisSelectItem: css`
      display: flex;
      align-items: center;
      margin-right: 8px;
      width: 100%;  
      gap: 8px;
    `,
    axisSelectItemButton: css`
      flex-shrink: 0;
    `
  };
});

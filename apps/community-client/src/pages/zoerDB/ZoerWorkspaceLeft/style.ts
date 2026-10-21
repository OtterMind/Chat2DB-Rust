import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      flex-shrink: 0;
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    header: css`
      flex-shrink: 0;
      display: flex;
      justify-content: space-between;
      align-items: center;
      box-sizing: border-box;
      padding: 0px 8px;
      height: 36px;
      line-height: 32px;
      font-size: 14px;
      font-weight: 500;
      box-sizing: border-box;
      border-bottom: 1px solid ${token.colorBorderLayout};
    `,
    headerTitle: css``,
    tableBox: css`
      flex: 1;
      overflow: auto;
    `,
    tableItem: css`
      padding: 8px 12px;
      cursor: pointer;
      display: flex;
      align-items: center;
      gap: 8px;

      &:hover {
        background-color: ${token.colorPrimaryBgHover};
      }
    `,
    activeTableItem: css`
      background-color: ${token.colorPrimaryBgHover};
    `,
    textContent: css`
      flex: 1;
      width: 0px;
      /* Truncate overflowing text. */
      text-overflow: ellipsis;
      overflow: hidden;
      white-space: nowrap;
    `,
    tableName: css`
      font-size: 14px;
      font-weight: 500;
      color: ${token.colorText};
    `,
    tableComment: css`
      font-size: 12px;
      color: ${token.colorTextDescription};
    `,
  };
});

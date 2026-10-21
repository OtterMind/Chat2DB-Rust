import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    header: css`
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 8px;
    `,
    scrollBox: css`
      flex: 1;
      height: 0px;
      overflow: auto;
    `,
    flex: css`
      height: 100%;
    `,
    table: css`
      padding: 0 12px;
      border-radius: 6px;
      border: 1px solid ${token.colorBorderSecondary};
      margin-bottom: 20px;
    `,
    tableItem: css`
      padding: 16px 0;
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 14px;
      line-height: 22px;
      &:not(:last-child) {
        border-bottom: 1px solid ${token.colorBorderSecondary};
      }
    `,
    itemName: css`
      font-weight: ${token.fontWeightStrong};
      width: 140px;
    `,
    itemDesc: css`
      flex: 1;
      display: flex;
      align-items: center;
    `,
    itemOpt: css`
      width: 120px;
      display: flex;
      justify-content: end;
      align-items: center;
    `,
    modalContent:css``,
    modalTitle: css`
    
      font-size: 15px;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 8px;
    `,
    modalForm:css``,
    modalDesc:css`
      color: ${token.colorTextSecondary};
      margin-bottom: 8px;
    `,
    modalInput:css``,
  };
});

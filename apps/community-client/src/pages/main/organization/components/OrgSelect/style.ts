import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      border-bottom: 1px solid ${token.colorBorderSecondary};
      padding: 8px 12px;
    `,

    item: css`
      display: inline-flex;
      align-items: center;
      justify-content: start;
      cursor: pointer;
      min-width: 160px;
      & svg {
        color: ${token.colorTextQuaternary};
      }
    `,
    dropItem: css`
      padding: 8px 10px;
      border-radius: 4px;
      &:hover {
        background-color: ${token.colorFillQuaternary};
      }
    `,
    dropItemChecked: css`
      background-color: ${token.colorFillQuaternary};
    `,
    itemLeft: css`
      display: flex;
      align-items: center;
      flex: 0 0 auto;
      margin-right: 60px;
      min-width: 160px;
      flex: 0 0 auto;
    `,
    itemName: css`
      margin-left: 12px;
      margin-right: 6px;
    `,
    itemTag: css`
      font-size: 10px;
      font-weight: ${token.fontWeightStrong};
      color: ${token.colorTextSecondary};
      border: 1px solid ${token.colorTextSecondary};
      border-radius: 2px;
      line-height: 10px;
      padding: 1px 2px;
    `,

    dropdownWrapper: css`
      padding: 16px;
      padding-bottom: 8px;
      box-shadow: ${token.boxShadow};
      background-color: ${token.colorBgBase};
      border-radius: 8px;
    `,
    dropdownTitle: css`
      font-size: 16px;
      font-weight: ${token.fontWeightStrong};
      margin-bottom: 16px;
    `,

    divide: css`
      margin: 8px 10px 6px 10px;
      height: 1px;
      background-color: ${token.colorBorderSecondary};
    `,
    dropdownCreate: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 8px 10px ;
      cursor: pointer;
      & svg {
        color: ${token.colorTextQuaternary};
      }
      &:hover {
        background-color: ${token.colorFillQuaternary};
      }
    `,
    dropdownCreateIcon: css`
      width: 32px;
      height: 32px;
      color: ${token.colorTextTertiary};
      border-radius: 4px;
      background-color: ${token.colorFillTertiary};
      display: flex;
      justify-content: center;
      align-items: center;
    `,
  };
});

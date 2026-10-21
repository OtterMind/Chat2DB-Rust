import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    container: css`
    `,
    selectDatabase: css`
      display: flex;
      justify-content: space-between;
      /* align-items: center; */
    `,
    syncIcon: css`
      padding-top: 48px;
      width: 120px;
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    leftBox: css`
      flex: 1;
      display: flex;
      justify-content: center;
    `,
    rightBox: css`
      flex: 1;
      display: flex;
      justify-content: center;
    `,
    leftFrom: css`
      width: 100%;
    `,
    rightFrom: css`
      width: 100%;
    `,
    databaseMessage: css`
      margin-top: 20px;
    `,
    dataSourceLabel: css`
      display: flex;
      gap: 5px;
      align-items: center;
    `,
    formItem: css`
      .ant-form-item-control-input-content{
        display: flex;
        align-items: center;
        gap: 10px;
      }
    `,
    fromTitle: css`
      font-size: 16px;
      font-weight: bold;
      margin-bottom: 10px;
    `,
  };
});

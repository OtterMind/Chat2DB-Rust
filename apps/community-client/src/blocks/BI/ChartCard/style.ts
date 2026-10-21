import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
    return {
      chatCard: css`
        display: flex;
        flex-direction: column;
        border-radius: 8px;
        height: 100%;
        box-sizing: border-box;
        background-color: ${token.colorBgBase};
      `,
      body: css`
        flex: 1;
        height: 0px;
      `,
      header: css`
        height: 26px;
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 4px 6px 0px 6px;
      `,
      title: css`
        flex: 1;
        width: 0px;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-weight: bold;
        margin-right: 20px;
        height: 26px;
        line-height: 26px;
      `,
      action: css`
        flex-shrink: 0;
        display: flex;
        align-items: center;
        gap: 5px;
      `,
      input: css`
        width: 100%;
        height: 100%;
      `,
      errorComment: css`
        display: flex;
        align-items: center;
        justify-content: center;
        flex-direction: column;
        gap: 10px;
        color: ${token.colorError};
        font-size: 12px;
        margin-top: 10px;
        height: 100%;
      `,
    };
});

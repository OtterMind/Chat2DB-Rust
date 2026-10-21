import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    wrapper: css`
      padding: 72px 60px;
      overflow-y: auto;
      height: 100vh;
    `,
    top: css`
      display: flex;
      justify-content: space-between;
      margin-bottom: 45px;
    `,

    image: css`
      width: 88px;
      height: 88px;
    `,
    title: css`
      font-size: 24px;
      line-height: 100%;
      font-weight: ${token.fontWeightStrong};
    `,
    version: css`
      color: ${token.colorPrimary};
      background-color: ${token.colorPrimaryBg};
      padding: 2px 4px;
      font-size: 14px;
      line-height: 14px;
      display: flex;
      justify-content: center;
      align-items: center;
    `,
    desc: css`
      color: ${token.colorTextTertiary};
    `,
    downCount: css`
      display: flex;
      align-items: center;
      gap: 2px;
      color: ${token.colorTextTertiary};
      line-height: 100%;
    `,
    button: css``,

    content: css`
      margin-bottom: 120px;
      /* position: relative; */
    `,
    refresh: css`
      position: absolute;
      right: 0;
      top: -55px;
    `,
  };
});

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    container: css`
      display: flex;
      flex-direction: column;
      gap: 4px;
    `,
    analyzeContent: css`
      max-width: 100%;
    `,
    hiddenContent: css`
      display: none;
    `,
    analyze: css`
      display: flex;
      align-items: center;
    `,
    analyzeCenter: css`
      display: flex;
      align-items: center;
      /* height: 16px; */
      padding: 6px 10px 6px 6px;
      gap: 6px;
      border-radius: 6px;
      border: 1px solid ${token.colorBorderSecondary};
      user-select: none;
      cursor: pointer;
    `,
    prefix: css`
      display: flex;
      align-items: center;
    `,
    succeedIcon: css`
      color: ${token.colorTextSecondary};
    `,
    text: css`
      font-size: 12px;
      line-height: 16px;
      color: ${token.colorTextSecondary};
    `,
    unfold: css`
      transform: rotate(0);
      transition: transform 0.3s;
      color: ${token.colorTextSecondary};
    `,
    fold: css`
      transform: rotate(-90deg);
      transition: transform 0.3s;
      color: ${token.colorTextSecondary};
    `,
  };
});

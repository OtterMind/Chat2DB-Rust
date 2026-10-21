import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    aiEntryButton: css`
      width: fit-content;
      height: 24px;
      display: flex;
      align-items: center;
      gap: 4px;
      cursor: pointer;
      background: radial-gradient(204.52% 161.18% at 0% 0%, #FF4C33 0%, #FA8837 29.5%, #F218F4 72.5%, #AD00FF 100%);
      background-clip: text;
      -webkit-text-fill-color: transparent;
      user-select: none;
    `,
    icon: css`
      font-size: 20px;
    `,
  };
});

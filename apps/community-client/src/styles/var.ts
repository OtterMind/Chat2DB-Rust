import { css } from 'antd-style';

export const createVar = (_token) => ({
  singleLine: css`
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  `,
  lines: (lines) => css`
    word-break: break-word;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: ${lines};
    overflow: hidden;
    text-overflow: ellipsis;
  `,
  iconButton: css`
    display: flex;
    justify-content: center;
    align-items: center;
    height: 32px;
    width: 32px;
    border-radius: 4px;
    cursor: pointer;
    color: var(--color-text-45);
    &:hover {
      background-color: var(--color-bg-hover);
    }
    i {
      font-size: 22px;
    }
  `,
  fillAbsolute: css`
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
    bottom: 0;
  `,
  button: css`
    display: flex;
    align-items: center;
    span {
      font-size: 12px;
      font-weight: 400;
    }
  `,
  docEnBreak: css`
    word-break: break-all;
  `,
  loadingAnimation: css`
    @keyframes loadingAnimation {
      0% {
        transform: rotate(0deg);
      }
      100% {
        transform: rotate(360deg);
      }
    }
  `,
});

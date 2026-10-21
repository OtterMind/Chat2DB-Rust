import { createStyles, keyframes } from 'antd-style';
export const useStyles = createStyles(({ css, token }) => {
  const rotateGradient = keyframes`
    0% {
      background-position: 0% 50%;
    }
    50% {
      background-position: 100% 50%;
    }
    100% {
      background-position: 0% 50%;
    }
  `;
  return {
    chartCardBox: css``,
    chartCardList: css`
      height: 100%;
      overflow-y: auto;
      overflow-x: hidden;
      box-sizing: border-box;
    `,
    emptyPage: css`
      width: 100%;
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      height: 100%;
      gap: 24px;
    `,
    aiChart: css`
      display: flex;
      align-items: center;
      padding: 4px 15px;
      height: 100%;
      box-sizing: border-box;
      cursor: pointer;
      background-color: ${token.colorBgBase};
      &:hover {
        filter: brightness(1.1);
      }
    `,
    aiChartBox: css`
      border: 1px solid transparent;
      border-radius: ${token.borderRadius}px;
      background-clip: padding-box, border-box;
      background-origin: padding-box, border-box;
      background-image: linear-gradient(to right, ${token.colorBgLayout}, ${token.colorBgLayout}),
        radial-gradient(101.76% 204.52% at 0% 0%, #ff4c33 0%, #fa8837 29.5%, #f218f4 72.5%, #ad00ff 100%);
      background-size: 200% 200%;
      animation: ${rotateGradient} 3s linear infinite;
      overflow: hidden;
    `,
    aiStar: css`
      width: 24px;
      height: 24px;
      cursor: pointer;
    `,
    aiChartText: css`
      background: linear-gradient(to right, #ff4c33 25%, #fa8837 50%, #f218f4 75%, #ad00ff 100%);
      background-clip: text;
      -webkit-text-fill-color: transparent;
    `,
    gridBox: css`
      background-color: ${token.colorBgBase};
      position: relative;
      padding: 2px;
      border-radius: 8px;
      box-sizing: border-box;
      &:hover {
        padding: 0px;
        border: 2px solid ${token.colorPrimaryBorderHover};
        .react-resizable-handle-se {
          opacity: 1;
        }
        .dragGripper {
          display: block !important;
        }
      }
      .react-resizable-handle {
        background-image: none;
      }
      .react-resizable-handle-se {
        opacity: 0;
        border-radius: 50%;
        width: 16px;
        height: 16px;
        border: 2px solid ${token.colorBgBase};
        background-color: ${token.colorPrimary};
        position: absolute;
        bottom: -6px !important;
        right: -6px !important;
        cursor: nwse-resize !important;
        &::after {
          display: none;
        }
        &:hover {
          opacity: 1;
        }
      }
    `,
    gridBoxActive: css`
      padding: 0px;
      border: 2px solid ${token.colorPrimary} !important;
      .react-resizable-handle-se {
        opacity: 1;
      }
      .dragGripper {
        display: block !important;
      }
    `,
    dragHandleBox: css`
      height: 10px;
      display: flex;
      justify-content: center;
      align-items: center;
      position: absolute;
      top: 2px;
      left: 0;
      right: 0;
      bottom: 0;
      height: 8px;
      .dragGripper{
        display: none;
        color: ${token.colorTextSecondary};
      }
      &:hover {
        .dragGripper {
          display: block;
        }
      }
    `,
  };
});

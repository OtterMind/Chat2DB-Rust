import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }, direction: 'vertical' | 'horizontal') => {
  return {
    splitPaneUnpack: css`
      position: relative;
      &:hover .operatingHandle {
        display: flex;
      }
      &:hover .operatingHandleHorizontal {
        display: flex;
      }
      .operatingHandleBox {
        position: absolute;
        ${direction === 'vertical'
          ? `
          height: 32px;
          top: -24px;
          width:60px;
          left: 50%;
          transform: translateX(-50%);
        `
          : `
          width: 32px;
          right: -24px;
          height:50px;
          top: 50%;
          transform: translateY(-100%);
        `}
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 2;
        &:hover .operatingHandle {
          display: flex;
        }
      }
      .operatingHandleBoxRight {
        right: -18px;
        top: 50%;
        transform: translateY(-50%);
        left: auto; // Restore the default value.
        height: 40px;
        width: 20px;
      }
      .operatingHandleBoxLeft {
        left: -18px;
        top: 50%;
        transform: translateY(-50%);
        right: auto; // Restore the default value.
        height: 40px;
        width: 20px;
      }
      .operatingHandleBoxTop {
        top: -18px;
        left: 50%;
        transform: translateX(-50%);
        bottom: auto; // Restore the default value.
        height: 20px;
        width: 40px;
      }
      .operatingHandleBoxBottom {
        bottom: -18px;
        left: 50%;
        transform: translateX(-50%);
        top: auto; // Restore the default value.
        height: 20px;
        width: 40px;
      }
      .operatingHandle {
        display: none;
        align-items: center;
        justify-content: center;
        ${direction === 'vertical'
          ? `
          height: 14px;
          padding: 0px 10px;
          border-radius: 6px 6px 0px 0px;
          `
          : `
          width: 14px;
          padding: 10px 0px;
          border-radius: 0px 6px 6px 0px;
          
          `}
        background-color: ${token.colorFillSecondary};
        color: ${token.colorTextSecondary};
        cursor: pointer;
        &:hover {
          background-color: ${token.colorFillTertiary};
          color: ${token.colorPrimary};
        }
        &:active {
          filter: brightness(1.1);
        }
      }
      .operatingHandleIcon {
        transform: ${direction === 'vertical' ? 'rotate(0deg)' : 'rotate(90deg)'};
        transition: transform 0.2s;
      }
    `,
  };
});

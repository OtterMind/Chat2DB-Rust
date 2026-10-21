import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    modalOverlay: css`
      position: fixed;
      top: 0;
      left: 0;
      width: 100vw;
      height: 100vh;
      z-index: 1000;
      .modal-container{
        position: relative;
      }
      .react-resizable-handle-e,
      .react-resizable-handle-s,
      .react-resizable-handle-se,
      .react-resizable-handle-sw,
      .react-resizable-handle-nw,
      .react-resizable-handle-ne,
      .react-resizable-handle-n,
      .react-resizable-handle-w {
        position: absolute;
        transform: none;
        padding: 0px;
        background-image: none;
        width: auto;
        height: auto;
      }
      .react-resizable-handle-n {
        left: 10px;
        right: 10px;
        top: -5px;
        height: 10px;
        margin-left: 0px;
      }
      .react-resizable-handle-e {
        top: 0;
        bottom: 10px;
        right: -5px;
        width: 10px;
        margin-top: 0px;
      }
      .react-resizable-handle-s{
        height: 10px;
        bottom: -5px;
        left: 0px;
        right: 10px;
        margin-left: 0px;
      }
      .react-resizable-handle-w{
        width: 10px;
        top: 10px;
        bottom: 10px;
        left: -5px;
        margin-top: 0px;
      }
      .react-resizable-handle-ne,.react-resizable-handle-se,.react-resizable-handle-sw,.react-resizable-handle-nw {
        height: 16px;
        width: 16px;
      }
      .react-resizable-handle-ne {
        transform: translate(6px,-6px);
      }
      .react-resizable-handle-nw {
        transform: translate(-6px,-6px);
      }
      .react-resizable-handle-se {
        transform: translate(6px,6px);
      }
      .react-resizable-handle-sw {
        transform: translate(-6px,6px);
      }
      .dragHandle {
        cursor: move;
      }
    `,
    modalContent: css`
      box-sizing: border-box;
      overflow: hidden;
      position: relative;
      background-color: ${token.colorBgBase};
      box-shadow: 0px 4px 36px 0px rgba(0, 0, 0, 0.10);
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: 8px;
    `,
    dragHandleBox: css`
        display: flex;
        justify-content: center;
        align-items: center;
        position: absolute;
        top: 0px;
        left: 0;
        right: 0;
        bottom: 0;
        height: 12px;
        cursor: move;
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

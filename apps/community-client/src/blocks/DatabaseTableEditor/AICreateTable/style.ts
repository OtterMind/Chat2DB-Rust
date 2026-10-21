import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    draggableModal: css`
      position: absolute;
      .ant-modal-content{
        border: 0px !important;
      }
    `,
    aiCreateTable: css``,
    aiCreateTableBox: css`
      padding: 8px;
      height: 100%;
      display: flex;
      flex-direction: column;
      box-sizing: border-box;
    `,
    chatInput: css`
      padding: 6px 8px;
      border: 0px;
      border-radius: 6px;
      background-color: ${token.colorFillQuaternary};
      background-image: none;
      animation: none;
    `,
    inputCenter: css`
      width: 100%;
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
    `,
    inputLeftAddons: css`
      width: 20px;
      height: 20px;
      border-radius: 4px;
      position: relative;
      overflow: hidden;
      &::after {
        position: absolute;
        content: '';
        display: block;
        width: 50px;
        height: 50px;
        background: radial-gradient(circle at 50% 50%, #FF4C33 0%, #FA8837 33%, #F218F4 61%, #AD00FF 100%);
        top: -6px;
        left: -27px;
      }
    `,
    inputLeftAddonsBox: css`
      display: flex;
      align-items: center;
      justify-content: center;
      height: 30px;
    `,
    icon: css`
      position: absolute;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      z-index: 1;
      color: ${token.colorWhite};
    `,
    executeSQLBox: css`
      margin-top: 8px;
      flex: 1;
      height: 0px;
      cursor: auto;
    `,
    executeSQLBoxHidden: css`
      display: none;
    `,
    executeSQL: css`
      height: 100%;
      box-sizing: border-box;
    `,
  };
});

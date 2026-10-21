import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    treeBox: css`
      display: flex;
      flex-direction: column;
      font-size: 14px;
      overflow-x: auto;
      overflow-y: hidden;
      /* position: relative; */

      .ant-tree {
        background-color: transparent;
      }

      .ant-tree-switcher {
        display: none;
      }

      .ant-tree-list {
        width: fit-content;
        min-width: 100%;
        position: inherit !important;
      }

      .ant-tree-list-holder {
        min-width: fit-content;
        width: 100%;
        & > div {
          position: inherit !important;
          overflow: visible !important;
        }
      }

      .ant-tree-list-holder-inner {
        width: 100%;
        position: inherit !important;
      }

      .ant-tree-treenode {
        /* width: max-content; */
        min-width: 100%;
      }

      .ant-tree-node-content-wrapper {
        white-space: nowrap;
      }

      .ant-tree-list-scrollbar-thumb {
        background-color: ${token.colorFill} !important;
        transition: background-color 0.1s ease;
      }
      .ant-tree .ant-tree-node-content-wrapper.ant-tree-node-selected {
        background-color: ${token.colorPrimaryBgHover};
      }
    `,
    spinBox: css`
      display: flex;
      justify-content: center;
      align-items: center;
      height: 100%;
    `,
    switcherIcon: css`
      color: ${token.colorTextQuaternary};
    `,
    unfoldSwitcherIcon: css`
      transform: rotate(90deg);
    `,
  };
});

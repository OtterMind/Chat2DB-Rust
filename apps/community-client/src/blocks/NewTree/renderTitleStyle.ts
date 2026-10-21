import { createStyles } from 'antd-style';
import { hexToRgba } from '@/utils/color'

export const useStyles = createStyles(({ css, token }) => {
  const colorBorderSecondary50 = hexToRgba(token.colorBorderSecondary, 50);

  return {
    treeNodeMask: css`
      position: absolute;
      top: 0px;
      left: 0px;
      right: 0px;
      bottom: 0px;
    `,
    customTitle: css`
      display: flex;
      align-items: center;
      /* width: fit-content; */
      width: max-content;
      padding-right: 8px;
      height: 100%;
      input {
        height: 22px;
        box-sizing: border-box;
        border-radius: 3px;
        padding: 4px;
        position: relative;
        z-index: 1;
      }
    `,
    originalTitle: css`
      display: flex;
      align-items: center;
      max-width: 100%;
      min-width: 0px !important;
      width: fit-content !important;
      white-space: nowrap;
      flex-shrink: 0;
      padding: 0px !important;
      user-select: none;
    `,
    treeNodeDescribe: css`
      margin-left: 6px;
      font-size: 12px;
      color: ${token.colorTextQuaternary};
      white-space: nowrap;
      flex-shrink: 0;
      user-select: none;
    `,
    treeNodeCount: css`
      margin-left: 6px;
      font-size: 12px;
      color: ${token.colorTextQuaternary};
      white-space: nowrap;
      flex-shrink: 0;
      user-select: none;
    `,
    filtration: css`
      flex-shrink: 0;
      white-space: nowrap;
      width: fit-content;
      position: relative;
      z-index: 1;
      height: 16px;
      margin-left: 8px;
      line-height: 16px;
      padding: 0px 3px;
      border-radius: 4px;
      font-size: 12px;
      border: 1px solid ${colorBorderSecondary50};
      color: ${token.colorTextQuaternary};
      &:hover {
        color: ${token.colorText};
        border: 1px solid ${token.colorBorder};
      }
    `,
    switcherIcon: css`
      color: ${token.colorTextQuaternary};
    `,
    unfoldSwitcherIcon: css`
      transform: rotate(90deg);
    `,
    switcherIconBox: css`
      display: flex;
      align-items: center;
      justify-content: center;
      width: 20px;
      height: 100%;
      flex-shrink: 0;
      position: relative;
      z-index: 1;
    `,
    customIconBox: css`
      display: flex;
      align-items: center;
      justify-content: center;
      width: 22px;
      height: 24px;
      flex-shrink: 0;
      transform: translateX(-2px);
    `,
    customizeIconIsLeaf: css`
      color: ${token.colorTextQuaternary};
    `,
    customizeIcon: css`
      color: ${token.colorTextTertiary};
    `,
  };
});

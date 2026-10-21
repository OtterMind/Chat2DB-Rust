import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
  container: css`
    width: 100%;
    height: 100%;
  `,
  customPanel: css`
    padding: 4px;
    background: transparent;
  `,
  erModal: css`
    flex: 1;
    width: 100%;
    height: 0px;
  `,
  consoleERModal: css`
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  `,
  toolBarList: css`
    height: 30px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid ${token.colorBorderLayout};
  `,
  toolBarLeft: css`
    display: flex;
    align-items: center;
  `,
  toolBarRight: css`
    display: flex;
    align-items: center;
  `,
  toolBarItem: css`
    flex-shrink: 0;
    height: 100%;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 0px 4px;
    /* border-right: 1px solid ${token.colorBorderLayout}; */
  `,
}));

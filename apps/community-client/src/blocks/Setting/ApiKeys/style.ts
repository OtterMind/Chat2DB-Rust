import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    apiKeys: css`
      height: 100%;
      display: flex;
      flex-direction: column;
    `,
    apiKeysHeader: css`
      flex-shrink: 0;
    `,
    apiKeysBody: css`
      flex: 1;
      height: 0px;
      display: flex;
      flex-direction: column;
    `,
    titleBox: css`
      flex: 1;
      display: flex;
      align-items: center;
      gap: 12px;
      font-size: 16px;
      font-weight: 600;
    `,
    createButton: css`
      display: flex;
      justify-content: flex-end;
      margin-bottom: 15px;
      flex-shrink: 0;
    `,
    antdTableBox: css`
      flex: 1;
      height: 0px;
    `,
    modalBody: css`
      padding-top: 10px;
    `,
    apiKeyBox: css`
      display: flex;
      gap: 4px;
    `,
    apiKeyText: css`
      flex: 1;
      width: 0px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    `,
    iconButton: css`
      flex-shrink: 0;
    `,
    deleteButton: css`
      cursor: pointer;
      color: ${token.colorPrimary};
      &:hover {
        color: ${token.colorPrimaryHover};
      }
    `,
    createSuccessTips: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
    `,
    CreateSuccess: css`
      font-size: 20px;
      color: ${token.colorPrimary};
    `,
    createSuccessTips1: css`
      color: ${token.colorTextSecondary};
    `,
    createSuccessTips2: css`
      color: ${token.colorTextSecondary};
    `,
    apiKey: css`
      display: flex;
      align-items: center;
      gap: 6px;
    `,
  };
});

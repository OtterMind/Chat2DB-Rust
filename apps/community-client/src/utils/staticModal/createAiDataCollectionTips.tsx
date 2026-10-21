import i18n from '@/i18n';
import { v4 as uuid } from 'uuid';
import { useWorkspaceStore } from '@/store/workspace';
import { IconfontSvg, staticModal } from '@chat2db/ui';
import ModalTitle from '@/components/Modal/ModalTitle';
import { WorkspaceTabType, DatabaseTypeCode } from '@/constants';

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => {
  return {
    contentBox: css`
      padding-left: 40px;
      color: ${token.colorTextSecondary};
    `,
    subheading: css`
      display: flex;
      align-items: center;
      margin-top: 8px;
      color: ${token.colorTextTertiary};
      cursor: pointer;
      &:hover {
        color: ${token.colorPrimary};
      }
    `,
    tipsIcon: css`
      margin-left: 4px;
    `,
  };
});

const createAiDataCollectionTips = (extraParams) => {
  if ([DatabaseTypeCode.MONGODB, DatabaseTypeCode.REDIS].includes(extraParams?.databaseType)) {
    return;
  }
  staticModal.confirm({
    icon: null,
    closable: true,
    title: <ModalTitle title={i18n('workspace.create.aiDataCollection.title')} iconCode="icon-sparkles" />,
    content: <ModalContent />,
    width: 600,
    okText: i18n('common.button.affirm'),
    cancelText: i18n('common.button.cancel'),
    onOk: () => {
      const title = [extraParams?.dataSourceName, 'tables'].filter(Boolean).join('-');
      useWorkspaceStore.getState().addWorkspaceTab({
        id: uuid(),
        type: WorkspaceTabType.ViewAllTable,
        title,
        uniqueData: {
          ...extraParams,
        },
      });
    },
  });
  // useWorkspaceStore.getState().createAiDataCollectionTipsCount;
  // if (useWorkspaceStore.getState().createAiDataCollectionTipsCount < 3) {
  //   useWorkspaceStore.getState().increaseCreateAiDataCollectionTipsCount();
  // }
};

export const ModalContent = () => {
  const { styles } = useStyles();
  return (
    <div className={styles.contentBox}>
      <span>{i18n('workspace.create.aiDataCollection.describe')}</span>
      <a
        className={styles.subheading}
        href="https://docs.chat2db-ai.com/docs/ai-chat/ai-data-collection"
        target="_blank"
        rel="noopener noreferrer"
      >
        {i18n('workspace.aiDataCollection.noData.tip')}
        <IconfontSvg className={styles.tipsIcon} size="md" code="icon-question-mark-circle" />
      </a>
    </div>
  );
};

export default createAiDataCollectionTips;

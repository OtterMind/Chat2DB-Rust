// Pinned form
import { useState } from 'react';
import { Button, Input } from 'antd';
import i18n from '@/i18n';
import { openModal } from '@/store/common/components';
import ModalTitle from '@/components/Modal/ModalTitle';
import { createStyles, css } from 'antd-style';
import aiDataCollectionService from '@/service/aiDataCollection';
import { CollectionSource, DataCollectionElementType } from '@/constants/aiDataCollection';
import { useWorkspaceStore } from '@/store/workspace';
import { v4 as uuid } from 'uuid';
import { WorkspaceTabType } from '@/constants';

export const useStyles = createStyles(({ token }) => {
  return {
    modalContent: css`
      padding-top: 20px;
    `,
    prefixIcon: css`
      color: ${token.colorTextTertiary};
    `,
    tips: css`
      margin: 6px 0px;
      font-size: 12px;
      color: ${token.colorTextTertiary};
    `,
    footer: css`
      display: flex;
      justify-content: flex-end;
      margin-top: 24px;
      gap: 8px;
    `,
  };
});

export const openCreateAiDataCollectionModal = (treeNodeData, loadData) => {
  openModal({
    width: '700px',
    headerIconCode: 'icon-colourful-folder-close-ai',
    title: (
      <ModalTitle
        title={i18n('workspace.aiDataCollection.title')}
        tips={{
          text: i18n('workspace.aiDataCollection.doc'),
          url: 'https://docs.chat2db-ai.com/docs/ai-chat/ai-data-collection',
        }}
      />
    ),
    content: (
      <CreateAiDataCollectionModalContent treeNodeData={treeNodeData} loadData={loadData} openModal={openModal} />
    ),
  });
};

export const CreateAiDataCollectionModalContent = (params: { treeNodeData: any; openModal: any; loadData: any }) => {
  const { treeNodeData, loadData } = params;
  const [value, setValue] = useState('');
  const { styles } = useStyles();
  const { addWorkspaceTab } = useWorkspaceStore((state) => {
    return {
      addWorkspaceTab: state.addWorkspaceTab,
    };
  });

  const {
    extraParams: { dataSourceId, dataSourceName },
    extraParams,
  } = treeNodeData;

  const onOk = () => {
    if (!value) {
      return;
    }
    const param = {
      title: value,
      dataSourceId,
      collectionSource: CollectionSource.DATA_SOURCE,
    };

    aiDataCollectionService.createAiDataCollection(param).then(() => {
      loadData(treeNodeData, {
        refresh: true,
      });
      openModal(false);
      addWorkspaceTab({
        id: uuid(),
        title: `${dataSourceName}-tables`,
        type: WorkspaceTabType.ViewAllTable,
        uniqueData: {
          ...extraParams,
          dataCollectionElementType: DataCollectionElementType.TABLE,
        },
      });
    });
  };

  const handleChange = (e) => {
    setValue(e.target.value);
  };

  return (
    <div className={styles.modalContent}>
      <Input
        size="large"
        placeholder={i18n('workspace.aiDataCollection.input.placeholder')}
        onChange={handleChange}
        value={value}
        // prefix={<IconfontSvg className={styles.prefixIcon} code="icon-folder-close-ai" />}
      />
      <div className={styles.tips}>{i18n('workspace.aiDataCollection.tips.content')}</div>
      <div className={styles.footer}>
        <Button
          onClick={() => {
            openModal(false);
          }}
        >
          {i18n('common.button.cancel')}
        </Button>
        <Button type="primary" onClick={onOk}>
          {i18n('common.button.affirm')}
        </Button>
      </div>
    </div>
  );
};

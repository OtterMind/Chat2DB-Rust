// Pinned form
import { openModal } from '@/store/common/components';
import i18n from '@/i18n';
import SchemaSync from '@/blocks/SchemaSync';
import { type ISelectDatabase } from '@/hooks/useSelectDatabase';

export const openSchemaSyncModal = (params: ISelectDatabase) => {
  const handleClose = () => {
    openModal(null);
  };

  openModal({
    width: '800px',
    title: i18n('workspace.syncStructure.title'),
    headerIconCode: 'icon-schema-sync',
    content: (
      <SchemaSync
        initSourceData={{
          ...params,
          selectDone: true,
        }}
        onClose={handleClose}
      />
    ),
  });
};

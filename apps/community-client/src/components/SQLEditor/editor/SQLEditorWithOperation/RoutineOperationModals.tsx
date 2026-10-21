import React, { forwardRef } from 'react';
import { Modal } from 'antd';
import i18n from '@/i18n';
import SQLPreview from '@/components/SQLPreview';
import { IBoundInfo } from '@/typings';
import SQLEditor, { SQLEditorRef } from '../SQLEditor';

interface RoutineExecutionModalState {
  open: boolean;
  title: string;
  sql: string;
}

interface RoutineMigrationModalState {
  open: boolean;
  title: string;
  sql: string;
  loading: boolean;
}

interface RoutineOperationModalsProps {
  editorId: string;
  dbInfo: IBoundInfo;
  executionModal: RoutineExecutionModalState;
  migrationModal: RoutineMigrationModalState;
  executionEditorRef: React.RefObject<SQLEditorRef>;
  onConfirmExecution: () => void;
  onCancelExecution: () => void;
  onConfirmMigration: () => void;
  onCancelMigration: () => void;
}

const RoutineOperationModals = ({
  editorId,
  dbInfo,
  executionModal,
  migrationModal,
  executionEditorRef,
  onConfirmExecution,
  onCancelExecution,
  onConfirmMigration,
  onCancelMigration,
}: RoutineOperationModalsProps) => {
  return (
    <>
      <Modal
        open={executionModal.open}
        title={executionModal.title}
        width={600}
        okText={i18n('common.button.confirm')}
        cancelText={i18n('common.button.cancel')}
        destroyOnClose
        onOk={onConfirmExecution}
        onCancel={onCancelExecution}
      >
        <div style={{ maxHeight: '60vh', overflow: 'auto' }}>
          <RoutineSqlEditor
            id={`${editorId}-routine-execution`}
            dbInfo={dbInfo}
            ref={executionEditorRef}
            sql={executionModal.sql}
          />
        </div>
      </Modal>
      <Modal
        open={migrationModal.open}
        title={migrationModal.title}
        width={760}
        okText={i18n('common.button.confirm')}
        cancelText={i18n('common.button.cancel')}
        confirmLoading={migrationModal.loading}
        destroyOnClose
        onOk={onConfirmMigration}
        onCancel={onCancelMigration}
      >
        <div style={{ maxHeight: '60vh', overflow: 'auto' }}>
          <SQLPreview sql={migrationModal.sql} source="routine-migration-modal" foldable={false} />
        </div>
      </Modal>
    </>
  );
};

const RoutineSqlEditor = forwardRef<
  SQLEditorRef,
  {
    id: string;
    dbInfo: IBoundInfo;
    sql: string;
  }
>((props, ref) => {
  return (
    <div style={{ height: 320, border: '1px solid var(--color-border)' }}>
      <SQLEditor id={props.id} ref={ref} dbInfo={props.dbInfo} active defaultValue={props.sql} action={() => {}} />
    </div>
  );
});

export default RoutineOperationModals;

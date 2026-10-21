import { memo } from 'react';
import { Modal } from 'antd';
import MonacoEditor from '@/components/MonacoEditor';

const TriggeredModal = memo<ITriggeredModal>(() => {
  return (
    <Modal
      title={`${data.key}-DDL`}
      open={monacoVerifyDialog}
      width="650px"
      maskClosable={false}
      onCancel={() => {
        setMonacoVerifyDialog(false);
      }}
      footer={false}
    >
      <div className={styles.monacoEditorBox}>
        <MonacoEditor id="edit-dialog" ref={monacoEditorRef} />
      </div>
    </Modal>
  );
});

export default TriggeredModal;

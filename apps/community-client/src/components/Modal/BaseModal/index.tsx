import React, { memo, useEffect, useMemo, useState } from 'react';
import { Modal } from '@chat2db/ui';
import { injectOpenModal } from '@/store/common/components';

export type IModalData =
  | {
      title?: React.ReactNode | string;
      headerIconCode?: string;
      width?: string;
      onOk?: () => void;
      footer?: React.ReactNode | false;
      content: React.ReactNode | false;
    }
  | null
  | false;

const BaseModal = memo(() => {
  const [open, setOpen] = useState(false);
  const [modalData, setModalData] = useState<IModalData>(null);

  const openModal = (params: IModalData) => {
    if (params === false) {
      setOpen(false);
    } else {
      setOpen(true);
      setModalData(params);
    }
  };

  useEffect(() => {
    injectOpenModal(openModal);
  }, []);

  const footer = useMemo(() => {
    if (modalData && modalData.footer) {
      return {
        footer: modalData.footer,
        onOk: modalData.onOk,
      };
    } else {
      return {
        footer: null,
      };
    }
  }, [modalData]);

  return (
    !!modalData && (
      <Modal
        title={modalData.title}
        open={open}
        width={modalData.width}
        maskClosable={false}
        headerBorder
        headerIconCode={modalData.headerIconCode}
        destroyOnClose
        onCancel={() => {
          setOpen(false);
        }}
        {...footer}
      >
        {modalData.content}
      </Modal>
    )
  );
});

export default BaseModal;

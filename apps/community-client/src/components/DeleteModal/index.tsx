import { memo, useEffect } from 'react';
import { Modal } from 'antd';
import { useGlobalStore } from '@/store/global';

interface IProps {
  className?: string;
}

export default memo<IProps>(() => {
  const [modal, contextHolder] = Modal.useModal();
  const setDeleteModal = useGlobalStore((s) => s.setDeleteModal);

  useEffect(() => {
    setDeleteModal(modal);
  }, []);

  return <>{contextHolder}</>;
});

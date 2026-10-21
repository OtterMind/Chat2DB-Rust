import DeleteModal from '@/components/DeleteModal';
import Modal from '@/components/Modal/BaseModal';
import SystemErrorMessage from '@/components/SystemErrorMessage';
import UnifiedConfirmationModal from '@/components/UnifiedConfirmationModal';

const GlobalComponentCommunity = () => {
  return (
    <>
      <SystemErrorMessage />
      <UnifiedConfirmationModal />
      <Modal />
      <DeleteModal />
    </>
  );
};

export default GlobalComponentCommunity;

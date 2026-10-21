import DeleteModal from '@/components/DeleteModal';
import Modal from '@/components/Modal/BaseModal';
import SystemErrorMessage from '@/components/SystemErrorMessage';
import UnifiedConfirmationModal from '@/components/UnifiedConfirmationModal';

// Some components that need to be initialized
const GlobalComponent = () => {
  return (
    <>
      <SystemErrorMessage />
      <UnifiedConfirmationModal />
      <Modal />
      <DeleteModal />
    </>
  );
};

export default GlobalComponent;

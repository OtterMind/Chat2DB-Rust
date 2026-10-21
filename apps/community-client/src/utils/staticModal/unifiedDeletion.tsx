import i18n from '@/i18n';
import { staticModal } from '@chat2db/ui';
import ModalTitle from '@/components/Modal/ModalTitle';

import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => {
  return {
    contentBox: css``,
  };
});

const openUnifiedDeletion = ({ title, okCallBack }) => {
  staticModal.confirm({
    icon: null,
    closable: true,
    title: <ModalTitle size="sm" title={title} iconCode="icon-trash" />,
    // content: <ModalContent />,
    width: 400,
    okText: i18n('common.button.affirm'),
    cancelText: i18n('common.button.cancel'),
    onOk: () => {
      okCallBack();
    },
  });
};

export const ModalContent = () => {
  const { styles } = useStyles();
  return <div className={styles.contentBox}>1</div>;
};

export default openUnifiedDeletion;

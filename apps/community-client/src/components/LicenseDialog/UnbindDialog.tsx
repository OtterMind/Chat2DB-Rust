import React from 'react';
import { Modal, ModalProps, TextArea } from '@chat2db/ui';
import { Button, Flex } from 'antd';
import { useStyles } from './style';
import i18n from '@/i18n';

interface UnbindDialogProps extends ModalProps {
  license: string;
  activationError: string | null;
  onLicenseChange: (value: string) => void;
  onSubmit: () => void;
  showErrorDetails: () => void;
  isLoading: boolean;
}

const UnbindDialog: React.FC<UnbindDialogProps> = ({
  open,
  license,
  activationError,
  onLicenseChange,
  onSubmit,
  showErrorDetails,
  onClose,
  isLoading,
  ...rest
}) => {
  const { styles } = useStyles();

  return (
    <Modal
      open={open}
      title={i18n('setting.license.unbindTitle')}
      titleDesc={i18n('setting.license.unbindDesc')}
      footer={null}
      headerBorder={true}
      centered
      destroyOnClose
      width={500}
      onClose={onClose}
      onCancel={onClose}
      {...rest}
    >
      <div className={styles.wrapper}>
        <Flex vertical gap={20}>
          <div className={styles.title}>
            {i18n('setting.license.enterLicenseKey')}
            <div className={styles.titleDesc}>{i18n('setting.license.enterLicenseKey.desc')}</div>
          </div>

          <div className={styles.content}>
            {/* <ActivationCodeInput onChange={onLicenseChange} /> */}
            <TextArea autoSize={{ minRows: 1, maxRows: 8 }} onChange={(e) => onLicenseChange(e.target.value)} />
            {activationError && (
              <div className={styles.errorMessage} onClick={showErrorDetails}>
                {activationError}
              </div>
            )}
          </div>
        </Flex>

        <Flex justify="flex-end" gap={8} style={{ marginTop: 20 }}>
          <Button onClick={onClose}>{i18n('common.button.cancel')}</Button>
          <Button danger type="primary" disabled={license.length < 16 || isLoading} onClick={onSubmit}>
            {i18n('setting.license.deactivate')}
          </Button>
        </Flex>
      </div>
    </Modal>
  );
};

export default UnbindDialog;

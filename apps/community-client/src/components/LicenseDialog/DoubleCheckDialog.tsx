import React, { useState, useEffect } from 'react';
import { Modal, ModalProps, staticMessage } from '@chat2db/ui';
import { Input, Flex, Button } from 'antd';
import { useStyles } from './style';
import LicenseService from '@/service/license';
import i18n from '@/i18n';
import { Dot } from 'lucide-react';

interface DoubleCheckDialogProps extends ModalProps {
  doubleCheckDisplayEmail: string | null;
  license: string;
  onClose: (e: React.SyntheticEvent) => void;
  onSubmit: (passcode: string) => void;
  isLoading: boolean;
}

const DoubleCheckDialog: React.FC<DoubleCheckDialogProps> = ({
  open,
  doubleCheckDisplayEmail,
  license,
  onClose,
  onSubmit,
  isLoading,
  ...rest
}) => {
  const { styles } = useStyles();
  const [countdown, setCountdown] = useState(0);
  const [passcode, setPasscode] = useState('');

  useEffect(() => {
    if (countdown > 0) {
      const timer = setTimeout(() => setCountdown(countdown - 1), 1000);
      return () => clearTimeout(timer);
    }
  }, [countdown]);

  const handleGetCode = () => {
    setCountdown(60);

    console.log('license', license);
    LicenseService.sendEmailLicense({ licenseKey: license }).then(() => {
      staticMessage.success('验证码已发送');
    });
  };

  return (
    <Modal
      open={open}
      title={i18n('setting.license.doubleCheckTitle')}
      titleDesc={i18n('setting.license.doubleCheckDesc')}
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
          {doubleCheckDisplayEmail ? (
            <>
              <div className={styles.title}>{i18n('setting.license.doubleCheckEmail')}</div>
              <div className={styles.content}>
                <Input disabled value={doubleCheckDisplayEmail} />
              </div>
              <div className={styles.title}>{i18n('setting.license.code')}</div>
              <div className={styles.content}>
                <Input
                  value={passcode}
                  onChange={(e) => setPasscode(e.target.value)}
                  placeholder={i18n('setting.license.pleaseEnterCode')}
                  addonAfter={
                    <Button size="small" type={'text'} onClick={handleGetCode} disabled={countdown > 0}>
                      {countdown > 0
                        ? i18n('setting.license.codeCountdown', countdown)
                        : i18n('setting.license.getCode')}
                    </Button>
                  }
                />
              </div>
              <Flex vertical gap={20}>
                <div className={styles.title}>{i18n('setting.license.offlineUsageNotes')}</div>
                <Flex vertical className={styles.tips}>
                  <Flex align="center">
                    <Dot size={20} className={styles.bulletPoint} />
                    <div>{i18n('setting.license.doubleCheckEmailDesc')}</div>
                  </Flex>
                  <Flex align="center">
                    <Dot size={20} className={styles.bulletPoint} />
                    <div>{i18n('setting.license.contactInfo')}</div>
                  </Flex>
                </Flex>
              </Flex>
            </>
          ) : (
            <>
              <div className={styles.title}>{i18n('setting.license.codeChannel')}</div>
              <div className={styles.content}>{i18n('setting.license.codeChannelDesc')}</div>
              <div className={styles.title}>{i18n('setting.license.code')}</div>
              <div className={styles.content}>
                <Input
                  value={passcode}
                  onChange={(e) => setPasscode(e.target.value)}
                  placeholder={i18n('setting.license.pleaseEnterCode')}
                />
              </div>
              <Flex vertical gap={20}>
                <div className={styles.title}>{i18n('setting.license.offlineUsageNotes')}</div>
                <Flex vertical className={styles.tips}>
                  <Flex align="center">
                    <Dot size={20} className={styles.bulletPoint} />
                    <div>{i18n('setting.license.contactInfo')}</div>
                  </Flex>
                </Flex>
              </Flex>
            </>
          )}
        </Flex>
        <Flex justify="flex-end" gap={8} style={{ marginTop: 20 }}>
          <Button onClick={onClose}>{i18n('common.button.cancel')}</Button>
          <Button type="primary" disabled={isLoading} onClick={() => onSubmit(passcode)}>
            {i18n('common.button.confirm')}
          </Button>
        </Flex>
      </div>
    </Modal>
  );
};

export default DoubleCheckDialog;

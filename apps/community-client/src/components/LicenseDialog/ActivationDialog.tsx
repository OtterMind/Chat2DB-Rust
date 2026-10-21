import React, { useEffect, useState } from 'react';
import { Modal, ModalProps, ActivationCodeInput, TextArea, Input, CopyButton, IconButton } from '@chat2db/ui';
import { Button, Flex, Popover, QRCode, Radio, Segmented, Spin } from 'antd';
import { Dot, LayoutDashboard, RefreshCcwIcon } from 'lucide-react';
import licenseService from '@/service/license';
import i18n from '@/i18n';
import { useStyles } from './style';
import { openWebPage } from '@/utils/url';

interface ActivationDialogProps extends ModalProps {
  license: string;
  activationError: string | null;
  appUrlConfig: any;
  onClose: (e: React.SyntheticEvent) => void;
  onLicenseChange: (v: string) => void;
  onSubmit: (activeType: 'online' | 'offline') => void;
  showErrorDetails: () => void;
  isLoading: boolean;
}

const ActivationDialog: React.FC<ActivationDialogProps> = ({
  open,
  license,
  activationError,
  appUrlConfig,
  onClose,
  onLicenseChange,
  onSubmit,
  showErrorDetails,
  isLoading,
  ...rest
}) => {
  const { styles } = useStyles();
  const [activeType, setActiveType] = useState<'online' | 'offline'>('online');
  const [deviceId, setDeviceId] = useState<string>('');
  const [deviceLoading, setDeviceLoading] = useState<boolean>(false);
  const [deviceType, setDeviceType] = useState<'QRCode' | 'Text'>('QRCode');

  useEffect(() => {
    if (activeType === 'offline' && !deviceId) {
      queryDeviceId();
    }
  }, [activeType]);

  const queryDeviceId = async () => {
    try {
      setDeviceLoading(true);
      const res = await licenseService.getDeviceId();
      setDeviceId(res);
    } catch (error) {
      console.log(error);
    } finally {
      setDeviceLoading(false);
    }
  };

  const renderButtonDisabled = () => {
    if (activeType === 'online') {
      return license.length < 16 || isLoading;
    }

    if (activeType === 'offline') {
      return license.length < 200 || deviceLoading || isLoading;
    }
  };

  return (
    <Modal
      open={open}
      title={i18n('setting.license.activationTitle')}
      titleDesc={i18n('setting.license.activationDesc')}
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
          <div className={styles.title}>{i18n('license.selectActivationType')}</div>
          <Flex gap={10}>
            <Radio.Group
              value={activeType}
              onChange={(e) => {
                setActiveType(e.target.value);
                onLicenseChange('');
              }}
            >
              <Radio value="online">{i18n('license.onlineActivation')}</Radio>
              <Radio value="offline">{i18n('license.offlineActivation')}</Radio>
            </Radio.Group>
          </Flex>
        </Flex>

        {activeType === 'online' && (
          <Flex vertical gap={20}>
            <div className={styles.title}>{i18n('setting.license.enterLicenseKey')}</div>
            <div className={styles.content}>
              <ActivationCodeInput onChange={onLicenseChange} />
              {activationError && (
                <div className={styles.errorMessage} onClick={showErrorDetails}>
                  {activationError}
                </div>
              )}
            </div>
          </Flex>
        )}

        {activeType === 'offline' && (
          <>
            <Flex vertical gap={20}>
              <div className={styles.title}>{i18n('license.deviceId')}</div>
              <Flex gap={8}>
                <Spin spinning={deviceLoading} wrapperClassName={styles.deviceId}>
                  <Input value={deviceId} />
                </Spin>
                {deviceId ? (
                  <Flex align="center" gap={4}>
                    <CopyButton copyContent={deviceId} />
                    <Popover
                      content={
                        <Flex vertical gap={4}>
                          <div style={{ maxWidth: '200px' }}>
                            {deviceType === 'QRCode' ? (
                              <QRCode value={deviceId} />
                            ) : (
                              <div style={{ wordBreak: 'break-all' }}>{deviceId}</div>
                            )}
                          </div>

                          <Segmented
                            block
                            options={['QRCode', 'Text']}
                            value={deviceType}
                            onChange={(v) => setDeviceType(v)}
                          />
                        </Flex>
                      }
                    >
                      <IconButton icon={LayoutDashboard} />
                    </Popover>
                  </Flex>
                ) : (
                  <IconButton icon={RefreshCcwIcon} onClick={queryDeviceId} />
                )}
              </Flex>
            </Flex>

            <Flex vertical gap={20}>
              <Flex vertical gap={4}>
                <Flex justify="space-between" align="center">
                  <div className={styles.title}>{i18n('license.deviceCertificate')}</div>
                  <div
                    className={styles.link}
                    onClick={() => {
                      openWebPage(`${appUrlConfig.CHAT2DB_APP_URL}/settings/deviceCer`);
                    }}
                  >
                    {i18n('license.getDeviceCertificate')}
                  </div>
                </Flex>
                <span style={{ opacity: 0.7, fontSize: 12 }}>{i18n('license.offlineCertificateInstruction')}</span>
              </Flex>
              <TextArea
                value={license}
                onChange={(v) => onLicenseChange(v.target.value)}
                maxLength={1024}
                autoSize={{ minRows: 4, maxRows: 10 }}
              />
            </Flex>
          </>
        )}

        <Flex vertical gap={20}>
          <div className={styles.title}>{i18n('setting.license.offlineUsageNotes')}</div>
          <Flex vertical className={styles.tips}>
            {activeType === 'offline' && (
              <>
                <Flex align="center">
                  <Dot size={20} className={styles.bulletPoint} />
                  <div className={styles.warning}>{i18n('license.offlineActivationAIWarning')}</div>
                </Flex>
                <Flex align="center">
                  <Dot size={20} className={styles.bulletPoint} />
                  <div className={styles.warning}>{i18n('license.offlineAIWarning')}</div>
                </Flex>
              </>
            )}
            <Flex align="center">
              <Dot size={20} className={styles.bulletPoint} />
              <div>{i18n('setting.license.deviceLimit')}</div>
            </Flex>
            <Flex align="center">
              <Dot size={20} className={styles.bulletPoint} />
              <Flex wrap="wrap">
                {i18n('setting.license.changeDeviceInstruction')}
                <div
                  className={styles.link}
                  onClick={() => {
                    if (activeType === 'online') {
                      openWebPage(`${appUrlConfig.DOCS_URL}/docs/start-guide/activate-unbind-license`);
                    } else {
                      openWebPage(`${appUrlConfig.DOCS_URL}/docs/start-guide/offline-activate`);
                    }
                  }}
                >
                  {i18n('setting.license.howToUnbind')}
                </div>
              </Flex>
            </Flex>
            <Flex align="center">
              <Dot size={20} className={styles.bulletPoint} />
              <div>{i18n('setting.license.contactInfo')}</div>
            </Flex>
          </Flex>
        </Flex>

        <Flex justify="flex-end" gap={8} style={{ marginTop: 20 }}>
          <Button onClick={onClose}>{i18n('common.button.cancel')}</Button>
          <Button type="primary" disabled={renderButtonDisabled()} onClick={() => onSubmit(activeType)}>
            {i18n('setting.license.activate')}
          </Button>
        </Flex>
      </div>
    </Modal>
  );
};

export default ActivationDialog;

import { useMemo, useState } from 'react';
import { Modal, ModalProps, IconfontSvg } from '@chat2db/ui';
import Logo from '@/components/Logo';
import { useStyles } from './style';
import { Button, Flex } from 'antd';
import { GuideDialogContent } from './data';
import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import { GuideDialogStatus, LicenseDialogType } from './type';
import { getAppConfig } from '@/constants/appConfig';
import LicenseService from '@/service/license';
import { isOfflineEnv } from '@/utils/env';
import { openWebPage } from '@/utils/url';

interface GuideDialogProps extends ModalProps {
  status: GuideDialogStatus;
}

const canNotCloseParams = {
  closable: false,
  maskClosable: false,
  keyboard: false,
};

const GuideDialog = (props: GuideDialogProps) => {
  const { open, status = GuideDialogStatus.FirstLogin, ...rest } = props;
  const { styles, cx } = useStyles();

  const content = useMemo(() => GuideDialogContent[status], [status]);

  const { appConfig, setOpenGuideDialog, setOpenLinenseDialog, setLicenseDialogType } = useGlobalStore((state) => ({
    setOpenGuideDialog: state.setOpenGuideDialog,
    appConfig: state.appConfig,
    setOpenLinenseDialog: state.setOpenLinenseDialog,
    setLicenseDialogType: state.setLicenseDialogType,
  }));

  const { setPricingModalStatus, queryCurUser } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    queryCurUser: s.queryCurUser,
  }));

  const curAppConfig = getAppConfig(appConfig.isCN);

  const [isPrimaryLoading, setIsPrimaryLoading] = useState(false);
  const [isSecondaryLoading, setIsSecondaryLoading] = useState(false);

  const handleHighLightSubTitle = (subTitle: string, highlightSubTitle: string) => {
    const parts = subTitle.split(new RegExp(`(${highlightSubTitle})`, 'gi'));

    return (
      <div>
        {parts.map((part, index) =>
          part === highlightSubTitle ? (
            <span key={index} className={styles.highlightSubTitle}>
              {part}
            </span>
          ) : (
            <span key={index}>{part}</span>
          ),
        )}
      </div>
    );
  };

  return (
    <Modal
      open={open}
      footer={null}
      width={540}
      maxHeight={'100vh'}
      centered
      maskClosable={false}
      styles={{ body: { paddingBlock: 0, paddingInline: 0 } }}
      {...rest}
      {...(isOfflineEnv ? canNotCloseParams : {})}
    >
      <div className={cx(styles.wrapper, status === GuideDialogStatus.Expired && styles.wrapperExpired)}>
        <Logo size={66} />
        <Flex vertical align="center" gap={8} className={styles.title}>
          <div style={{ textAlign: 'center', padding: '0 4px' }}>{content.title}</div>
          {handleHighLightSubTitle(content.subTitle, content.highlightSubTitle)}
        </Flex>

        <Flex align="center" gap={12} className={styles.divideline}>
          <div className={styles.leftDivideLine} />
          <div>{content.dividingLine}</div>
          <div className={styles.rightDivideLine} />
        </Flex>

        <Flex gap={16} vertical align="start">
          {content.feature.map((f, id) => (
            <Flex key={id} gap={16} justify="center">
              <Flex className={styles.featureIcon} justify="center" align="center">
                <IconfontSvg code={f.icon} className={styles.svgIcon} />
              </Flex>
              <Flex vertical gap={8} justify="center">
                <div className={styles.featureTitle}>{f.title}</div>
                <div className={styles.featureSubTitle}>{f.subtitle}</div>
              </Flex>
            </Flex>
          ))}
        </Flex>

        <Flex gap={16} justify="start" align="center" className={styles.primaryButton}>
          {content.primaryButton && (
            <Button
              type="primary"
              loading={isPrimaryLoading}
              onClick={async () => {
                setIsPrimaryLoading(true);
                try {
                  if (GuideDialogStatus.OfflineTrial === status) {
                    await LicenseService.startTrial();
                    await queryCurUser();
                    setOpenGuideDialog(false);
                  } else if (
                    [GuideDialogStatus.OfflineLicenseExpired, GuideDialogStatus.OfflineTrialExpired].includes(status)
                  ) {
                    const isCN = useGlobalStore.getState().appConfig.isCN;
                    if (isCN) {
                      const url = new URL(curAppConfig.CHAT2DB_PRICING_URL);
                      url.searchParams.set('productType', 'LOCAL');
                      openWebPage(url.toString());
                    } else {
                      const url = new URL(curAppConfig.WEBSITE_PRICING_URL);
                      url.searchParams.set('productType', 'LOCAL');
                      openWebPage(url.toString());
                    }
                  } else if (GuideDialogStatus.Expired === status) {
                    setPricingModalStatus(true);
                    setOpenGuideDialog(false);
                  } else {
                    setOpenGuideDialog(false);
                  }
                } finally {
                  setIsPrimaryLoading(false);
                }
              }}
            >
              {content.primaryButton.title}
            </Button>
          )}
          {content?.primaryButton2?.title && (
            <Button
              type="default"
              loading={isSecondaryLoading}
              onClick={() => {
                setIsSecondaryLoading(true);
                setOpenLinenseDialog(true);
                setLicenseDialogType(LicenseDialogType.Activation);
                setIsSecondaryLoading(false);
              }}
            >
              {content?.primaryButton2?.title}
            </Button>
          )}
        </Flex>

        <Button
          type="link"
          className={styles.secondaryButton}
          onClick={() => {
            openWebPage(curAppConfig.DOCS_URL);
          }}
        >
          {content.secondaryButton.title}
        </Button>
      </div>
    </Modal>
  );
};

export default GuideDialog;

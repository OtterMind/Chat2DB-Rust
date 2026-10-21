import { memo, useState } from 'react';
import { useStyles } from './style';
import { useOrgStore } from '@/store/organization';
import { useUserStore } from '@/store/user';
import { SubscriptionStatus } from '@/constants/user';
import i18n from '@/i18n';

import VipStatus from '../VipStatus';
import { Button } from 'antd';
import { OrganizationType } from '@/typings/enterprise/organization';
import { SubscriptionType } from '@/constants/subscriptionType';
import { Input, Modal } from '@chat2db/ui';
import misc from '@/service/misc';
import { isOfflineEnv } from '@/utils/env';
import { useGlobalStore } from '@/store/global';
import feedback from '@/utils/feedback';

export default memo((props: { openActivationCodeModal?: boolean; activationCode?: string }) => {
  const { openActivationCodeModal = false, activationCode: _activationCode } = props;
  const { styles } = useStyles();
  const { curOrgSubscription, curOrg } = useOrgStore((state) => ({
    curOrgSubscription: state.curOrgSubscription,
    curOrg: state.curOrg,
  }));
  const [activationCode, setActivationCode] = useState(_activationCode || '');
  const [openActivationCode, setOpenActivationCode] = useState(openActivationCodeModal);
  const [isProcessing, setIsProcessing] = useState(false);
  const { setPricingModalStatus, setSubscriptType } = useUserStore((state) => ({
    setPricingModalStatus: state.setPricingModalStatus,
    setSubscriptType: state.setSubscriptType,
  }));
  const { appConfig } = useGlobalStore((s) => ({
    appConfig: s.appConfig,
  }));
  const { isCN } = appConfig;

  const verified = curOrgSubscription?.status === SubscriptionStatus.Valid;

  return (
    <div className={styles.planBox}>
      <div className={styles.planHeader}>
        <div className={styles.planHeaderLeft}>
          <div className={styles.planTitle}>{i18n('price.text.plan')}</div>
          <VipStatus />
        </div>
        <div className={styles.planHeaderRight}>
          <Button
            type="primary"
            onClick={() => {
              setPricingModalStatus(true);
              setSubscriptType(
                curOrg?.type === OrganizationType.PERSONAL
                  ? SubscriptionType.PersonalUpdate
                  : SubscriptionType.TeamUpdate,
              );
            }}
          >
            {verified ? i18n('price.text.reorderPlan') : i18n('price.text.upgradePackage')}
          </Button>
          {!isOfflineEnv && isCN && (
            <Button
              type="primary"
              onClick={() => {
                setOpenActivationCode(true);
              }}
            >
              {i18n('price.text.activation')}
            </Button>
          )}
        </div>
      </div>
      {/* <div className={styles.degreeOfUse}>
        <div className={styles.degreeOfUseContainer}>
          <div className={styles.degreeOfUseTitle}>Number of data sources</div>
          <div className={styles.degreeOfUseValue}>1/infinite</div>
        </div>
        <div className={styles.degreeOfUseContainer}>
          <div className={styles.degreeOfUseTitle}>Number of users</div>
          <div className={styles.degreeOfUseValue}>1/infinite</div>
        </div>
        <div className={styles.degreeOfUseContainer}>
          <div className={styles.degreeOfUseTitle}>AI usage times</div>
          <div className={styles.degreeOfUseValue}>789/infinite</div>
        </div>
      </div> */}
      <Modal
        title={i18n('price.text.activation')}
        open={openActivationCode}
        onOk={() => {
          if (!isProcessing && activationCode.length > 0) {
            setIsProcessing(true);
            misc
              .activationCode({
                activationCode,
              })
              .then(() => {
                setOpenActivationCode(false);
                feedback.success(i18n('price.text.activationCodeSuccess'));

                setTimeout(() => {
                  window.history.replaceState({}, '', window.location.pathname);
                  window.location.reload();
                }, 1000);
              })
              .catch((error) => {
                feedback.error(error.errorMessage);
              })
              .finally(() => {
                setIsProcessing(false);
              });
          }
        }}
        onCancel={() => setOpenActivationCode(false)}
      >
        <Input
          value={activationCode}
          onChange={(e) => setActivationCode(e.target.value)}
          placeholder={i18n('price.text.activationCodePlaceholder')}
          maxLength={128}
        />
      </Modal>
    </div>
  );
});

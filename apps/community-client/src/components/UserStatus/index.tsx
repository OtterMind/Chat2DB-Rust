import { useOrgStore } from '@/store/organization';
import { useMemo } from 'react';
import { useStyles } from './style';
import { Button, Flex } from 'antd';
import { IconfontSvg } from '@chat2db/ui';
import dayjs from 'dayjs';
import { useUserStore } from '@/store/user';
import i18n, { i18nElement } from '@/i18n';
import { OrganizationType } from '@/typings/enterprise/organization';
import { useGlobalStore } from '@/store/global';
import { isOfflineEnv } from '@/utils/env';
import { GuideDialogStatus, LicenseDialogType } from '../GuideDialog/type';

const MONTH_DAYS = 30;
const TWO_WEEK_DAYS = 14;

const UserStatus = () => {
  const { styles, cx } = useStyles();

  const { setPricingModalStatus, curUser } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    curUser: s.curUser,
  }));

  const { curOrg, curOrgSubscription, orgList } = useOrgStore((state) => ({
    curOrg: state.curOrg,
    curOrgSubscription: state.curOrgSubscription,
    orgList: state.orgList,
  }));

  const {
    appConfig,
    isEmbedIframe,
    setOpenLinenseDialog,
    setOpenGuideDialog,
    setGuideDialogStatus,
    setLicenseDialogType,
  } = useGlobalStore((s) => ({
    appConfig: s.appConfig,
    isEmbedIframe: s.isEmbedIframe,
    setOpenLinenseDialog: s.setOpenLinenseDialog,
    setOpenGuideDialog: s.setOpenGuideDialog,
    setGuideDialogStatus: s.setGuideDialogStatus,
    setLicenseDialogType: s.setLicenseDialogType,
  }));
  const { isCN } = appConfig;

      // The trial and Pro indicators differ.
  const isFree = useMemo(() => !curOrg?.vip, [curOrg?.vip]);

  const remainDays = useMemo(() => {
        // Missing subscription information indicates a trial.
    if (!curOrgSubscription) {
      return MONTH_DAYS - dayjs().diff(dayjs(curOrg?.createTime), 'day');
    }
        // Calculate the validity period from subscription information.
    return dayjs(curOrgSubscription?.endTime).diff(dayjs(), 'day');
  }, [curOrgSubscription?.endTime]);

  if (isOfflineEnv) {
    const { trialStartTime, activated } = curUser || {};

      // Activated.
    if (activated) {
      return null;
    }

    // Trial not started.
    if (!trialStartTime) {
      return (
        <Flex className={cx(styles.wrapper)}>
          <Flex align="center" gap={4} flex={1}>
            <IconfontSvg className={styles.icon} code="icon-clock" />
            <div className={styles.text}>{i18n('userguide.vipStatus.offlineTrialFree')}</div>
          </Flex>
          <Button
            className={cx(styles.buttonRight)}
            type="primary"
            size="small"
            onClick={() => {
              setOpenGuideDialog(true);
              setGuideDialogStatus(GuideDialogStatus.OfflineTrial);
            }}
          >
            {i18n('userguide.vipStatus.offlineTrial')}
          </Button>
        </Flex>
      );
    }

      // Trial started.
    const remainTrialDays = TWO_WEEK_DAYS - dayjs().diff(dayjs(trialStartTime), 'day');
    return (
      <Flex className={cx(styles.wrapper)}>
        <Flex align="center" gap={4} flex={1}>
          <IconfontSvg className={styles.icon} code="icon-clock" />
          {remainTrialDays <= TWO_WEEK_DAYS && (
            <div className={styles.text}>
              {remainTrialDays > 0
                ? i18nElement('userguide.vipStatus.offlineTrialDays', <span>{remainTrialDays}</span>)
                : i18n('userguide.vipStatus.offlineTrialExpired')}
            </div>
          )}
        </Flex>
        <Button
          className={cx(styles.buttonRight)}
          type="primary"
          size="small"
          onClick={() => {
            setOpenLinenseDialog(true);
            setLicenseDialogType(LicenseDialogType.Activation);
          }}
        >
          {i18n('userguide.vipStatus.activate')}
        </Button>
      </Flex>
    );
  }

    // Hide when more than one month remains.
  if (remainDays > MONTH_DAYS) {
    if ((orgList || []).find((i) => i.type !== OrganizationType.TEAM)) {
      // return (
      //   <Flex className={cx(styles.wrapper, styles.teamWrapper)}>
      //     <Flex align="center" gap={4} flex={1}>
      //       <IconfontSvg className={styles.icon} code="icon-a-xunwen1" />
      //       <div className={styles.text}>{i18n('userguide.vipStatus.experienceTeam')}</div>
      //     </Flex>
      //     <Button
      //       className={cx(styles.buttonRight)}
      //       type="primary"
      //       size="small"
      //       onClick={() => {
      //         setOpenCreateOrJoinOrgDialog(true);
      //       }}
      //     >
      //       {i18n('userguide.vipStatus.createTeam')}
      //     </Button>
      //   </Flex>
      // );
    }

    return null;
  }

  // Do not show the 30-day countdown to overseas users.
  if (!isCN && remainDays > 0 && curOrg?.vip) {
    return null;
  }

  if (isEmbedIframe) {
    return null;
  }

  return (
    <div
      className={styles.wrapper}
      onClick={() => {
        setPricingModalStatus(true);
      }}
    >
      <Flex className={styles.left} align="center" gap={4}>
        {/* <IconfontSvg className={styles.icon} code="icon-clock" /> */}
        <div className={styles.edition}>{isFree ? 'Free' : 'Pro'}</div>
        {remainDays <= MONTH_DAYS && (
          <div className={styles.text}>
            {remainDays > 0
              ? i18nElement('userguide.vipStatus.trialDays', <span>{remainDays}</span>)
              : i18n('userguide.vipStatus.trial')}
          </div>
        )}
      </Flex>
      <Button className={styles.buttonRight} type="primary" size="small">
        {i18n('userguide.vipStatus.upgrade')}
      </Button>
    </div>
  );
};

export default UserStatus;

import { useMemo } from 'react';
import { Flex } from 'antd';
import { useStyles } from './style';
import { formatDate } from '@/utils/date';
import { useOrgStore } from '@/store/organization';
import { useUserStore } from '@/store/user';
import i18n from '@/i18n';
import { useGlobalStore } from '@/store/global';

export enum VIP_TYPE {
  'FOREVER',
  'PRO',
  // Trial and expiration states.
  'TRIAL',
}

export interface IVipStatus {
  size?: 'sm' | 'md' | 'lg';
  handleClick?: () => void;
}

const VipStatus = ({ size = 'md', handleClick }: IVipStatus) => {
  const { setPricingModalStatus, curUser } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    curUser: s.curUser,
  }));
  const { curIsPersonalOrg, curOrg, curOrgSubscription } = useOrgStore((state) => ({
    curOrgSubscription: state.curOrgSubscription,
    curOrg: state.curOrg,
    curIsPersonalOrg: state.curIsPersonalOrg,
  }));

  const { appConfig } = useGlobalStore((s) => ({
    appConfig: s.appConfig,
  }));
  const { isCN } = appConfig;

  // subscriptionType returned by the backend, such as Starter, Pro, or Pro2.
  const serverSubscriptionType = curUser?.subscriptionType;

  const handleVipType = () => {
    if (!curOrg?.vip || !curOrgSubscription) {
      return VIP_TYPE.TRIAL;
    }
    // 90 years.
    const NINETY_YEARS_IN_MS = 90 * 365.25 * 24 * 60 * 60 * 1000;
    if (curOrgSubscription?.endTime - curOrgSubscription?.startTime > NINETY_YEARS_IN_MS) {
      return VIP_TYPE.FOREVER;
    }
    return VIP_TYPE.PRO;
  };

  const vipType = useMemo(() => handleVipType(), [curOrg?.vip, curOrgSubscription?.endTime]);

  const { styles, cx } = useStyles({ size, vipType, isOverseas: !isCN });

  const renderEdition = () => {
    // Prefer subscriptionType returned by the backend.
    if (serverSubscriptionType) {
      return serverSubscriptionType;
    }

    // Fall back to the original rules when the backend omits subscriptionType.
    if (curIsPersonalOrg()) {
      if (vipType === VIP_TYPE.FOREVER) {
        return i18n('userguide.vipType.forever');
      } else if (vipType === VIP_TYPE.TRIAL) {
        return i18n('userguide.vipType.trial');
      } else {
        return i18n('userguide.vipType.pro');
      }
    } else {
      if (vipType === VIP_TYPE.PRO) {
        return i18n('userguide.vipType.team.pro');
      } else {
        return i18n('userguide.vipType.team.trial');
      }
    }
  };

  const renderValidDate = () => {
    // No organization is available.
    if (!curOrg && !curIsPersonalOrg()) {
      return i18n('userguide.vipStatus.noTeam');
    }

    if (vipType === VIP_TYPE.FOREVER) {
      return i18n('userguide.vipStatus.forever');
    } else if (vipType === VIP_TYPE.TRIAL) {
      return i18n('userguide.vipStatus.trial');
    } else {
      return (
        <>
          {i18n('userguide.vipStatus.pro') + ' : '}
          {formatDate(curOrgSubscription?.endTime) || '2024-05-01'}
        </>
      );
    }
  };

  const handleValidDate = () => {
    if (vipType === VIP_TYPE.TRIAL) {
      setPricingModalStatus(true);
      handleClick && handleClick();
    }
  };

  return (
    <Flex align="center" className={styles.wrapper}>
      <div className={cx(styles.edition)}>{renderEdition()}</div>
      {isCN && (
        <div className={styles.validDate} onClick={handleValidDate}>
          {renderValidDate()}
        </div>
      )}
    </Flex>
  );
};

export default VipStatus;

import { useEffect, useState } from 'react';
import PriceIntro from '../PriceIntro';
import PriceMain from '../PriceMain';
import { OrganizationType } from '@/typings/enterprise/organization';
import { useGlobalStore } from '@/store/global';
import i18n from '@/i18n';
import { PayStatus } from '@/constants/pricing';
import { useStyles } from './style';

interface PricePageProps {
  planId: string;
  seats: number;
  invitationCode: string;
  productType: TabType;
  subscriptionType: SubscriptionType;
}

export enum SubscriptionType {
  MONTH = 'MONTH',
  YEAR = 'YEAR',
  FOREVER = 'FOREVER',
}

export enum TabType {
  LOCAL = 'LOCAL',
  PERSONAL = 'PERSONAL',
  TEAM = 'TEAM',
}

const PricePage = ({
  seats,
  invitationCode,
  productType,
  subscriptionType = SubscriptionType.MONTH,
}: PricePageProps) => {
  const [tabIndex, setTabIndex] = useState<TabType | OrganizationType>(productType || TabType.PERSONAL);
  const { styles } = useStyles();

  const { payStatus } = useGlobalStore((s) => ({
    payStatus: s.payStatus,
  }));

  useEffect(() => {
    if (payStatus === PayStatus.PAY_SUCCESS) {
      window.location.href = '/settings/purchase';
    }
  }, [payStatus]);

  return (
    <div className={styles.pricePage}>
      <div className={styles.priceTitle}>{i18n('price.title')}</div>
      <div className={styles.priceContent}>
        <PriceIntro tabIndex={tabIndex} className={styles.priceIntro} />
        <PriceMain
          isSinglePage={true}
          tabIndex={tabIndex}
          onTabChange={(key) => setTabIndex(key)}
          productParams={{
            subscriptionType,
            productType,
            invitationCode,
            seats,
          }}
        />
      </div>
    </div>
  );
};

export default PricePage;

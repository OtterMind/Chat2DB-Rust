import { useEffect } from 'react';
import { PricePage } from '@/components/Price';
import { useGlobalStore } from '@/store/global';
import { getAllUrlParams } from '@/utils/url';
// import pricingServices from '@/service/pricing';
// import { useStyles } from './style';
// import i18n from '@/i18n';
// import { LoadingGracile } from '@chat2db/ui';
import { history } from 'umi';
import { useUserStore } from '@/store/user';
import { isEmbedIframePage } from '@/utils/iframe';

const Price = () => {
  // const { styles } = useStyles();
  const { appConfig } = useGlobalStore((state) => ({
    setOpenGuideDialog: state.setOpenGuideDialog,
    appConfig: state.appConfig,
    setOpenLinenseDialog: state.setOpenLinenseDialog,
    setLicenseDialogType: state.setLicenseDialogType,
  }));

  const { setPricingModalStatus } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
  }));

  const { planId, seats, invitationCode, subscriptionType, productType } = getAllUrlParams();

  useEffect(() => {
    if (!appConfig.isCN) {
      history.push('/');
      if (isEmbedIframePage()) {
        return;
      }
      setPricingModalStatus(true);
      // if (!planId) {
      //   history.push('/');
      //   return;
      // }
    }
  }, []);

  if (!appConfig.isCN) {
    return null;
    // return (
    //   <div className={styles.generatingOrder}>
    //     <LoadingGracile />
    //     {i18n('price.text.generatingOrder')}
    //   </div>
    // );
  }

  return (
    <div>
      <PricePage
        planId={planId}
        seats={seats}
        invitationCode={invitationCode}
        subscriptionType={subscriptionType}
        productType={productType}
      />
    </div>
  );
};

export default Price;

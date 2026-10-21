import PricePage from './PricePage';
import CNPriceModal from './PriceModal';
import OverseasPayModal from './OverseasPayModal';
import { SubscriptionType } from '@/constants/subscriptionType';
import { useGlobalStore } from '@/store/global';
import { isEmbedIframePage } from '@/utils/iframe';

const PriceModal = () => {
  const { isCN, isEmbedIframe } = useGlobalStore((s) => ({
    isCN: s.appConfig.isCN,
    isEmbedIframe: s.isEmbedIframe,
  }));

  if (isEmbedIframe || isEmbedIframePage()) {
    return null;
  }

  if (!isCN) {
    return <OverseasPayModal />;
  }
  return <CNPriceModal />;
};

export { SubscriptionType, PriceModal, PricePage };

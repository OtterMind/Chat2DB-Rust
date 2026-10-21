import UpdateDetection from '@/blocks/UpdateDetection';
import { PriceModal } from '@/components/Price';
import { runtimeEditionConfig } from '@/constants/runtimeEdition';

const CommercialGlobalComponentExtras = () => {
  return (
    <>
      {runtimeEditionConfig.autoUpdate && <UpdateDetection />}
      {runtimeEditionConfig.pricingAutoPopup && <PriceModal />}
    </>
  );
};

export default CommercialGlobalComponentExtras;

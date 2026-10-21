import { useEffect, useMemo, useState } from 'react';
import { Flex } from 'antd';
import { Modal } from '@chat2db/ui';
import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import { PayStatus } from '@/constants/pricing';
import PriceIntro from '../PriceIntro';
import PriceMain from '../PriceMain';
import { useStyles } from './style';
import i18n from '@/i18n';
import { OrganizationType } from '@/typings/enterprise/organization';

const PriceModal = () => {
  const [tabIndex, setTabIndex] = useState<OrganizationType>();

  const { styles } = useStyles();
  const { queryCurUser, pricingModalStatus, subscriptType, setPricingModalStatus } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    pricingModalStatus: s.pricingModalStatus,
    subscriptType: s.subscriptType,
    queryCurUser: s.queryCurUser,
  }));

  const { payStatus } = useGlobalStore((s) => ({
    payStatus: s.payStatus,
  }));

  useEffect(() => {
    if (payStatus === PayStatus.PAY_SUCCESS) {
      queryCurUser();
      setPricingModalStatus(false);
    }
  }, [payStatus]);

  const modalTitle = useMemo(() => {
    switch (subscriptType) {
      case 'PersonalUpdate':
      case 'TeamUpdate':
        return i18n('price.modal.title.upgrade');
      case 'TeamAddSeat':
        return i18n('price.modal.title.addSeat');
      default:
        return i18n('price.modal.title.upgrade');
    }
  }, [subscriptType]);

  return (
    <Modal
      className={styles.pricingModal}
      open={!!pricingModalStatus}
      centered
      maxHeight={'80vh'}
      width={'960px'}
      footer={null}
      padding={0}
      onCancel={() => {
        setPricingModalStatus(false);
      }}
      maskClosable={false}
      title={modalTitle}
      headerIconCode="icon-update"
      headerBorder
      destroyOnClose
    >
      <Flex>
        <PriceIntro tabIndex={tabIndex} />
        <PriceMain tabIndex={tabIndex} onTabChange={(key) => setTabIndex(key)} />
      </Flex>
    </Modal>
  );
};
export default PriceModal;

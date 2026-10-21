import { memo, useState, forwardRef, ForwardedRef, useImperativeHandle, useMemo, useEffect } from 'react';
import { useStyles } from './style';
import { Modal, IconfontSvg } from '@chat2db/ui';
import i18n from '@/i18n';
import { Flex, Tooltip, Select, InputNumber, Button, Input } from 'antd';
import { useOrgStore } from '@/store/organization';
import { OrganizationType } from '@/typings/enterprise/organization';
import pricingServices from '@/service/pricing';
import { IPriceData } from '@/typings/pricing';
import { PayType } from '@/constants/pricing';
import { formatPrice } from '@/utils/price';
import { useUserStore } from '@/store/user';
import { openWebPage } from '@/utils/url';
import { useGlobalStore } from '@/store/global';

const MIN_TEAM_USER_COUNT = 5;
const MAX_TEAM_USER_COUNT = 5 * 1000;

interface IProps {
  className?: string;
  priceData: IPriceData;
}

export interface TeamSeatModalRef {
  setSeatModalVisible: (value: boolean) => void;
  handleOpenStripePage: (payDetail: IPriceData) => void;
}

const TeamSeatModal = forwardRef((props: IProps, ref: ForwardedRef<TeamSeatModalRef>) => {
  const { priceData } = props;
  const { styles } = useStyles();
  const [seatModalVisible, setSeatModalVisible] = useState(false);
  const [selectOrg, setSelectOrg] = useState<number>();
  const [seatNumber, setSeatNumber] = useState<number>(MIN_TEAM_USER_COUNT);
  const [createOrderLoading, setCreateOrderLoading] = useState<boolean>(false);
  const [payDetail, setPayDetail] = useState<any>();
  const [inviteCode, setInviteCode] = useState<string>('');
  const { userOrgId } = useUserStore((s) => ({
    userOrgId: s.curUser?.currentOrganization.id,
  }));

  useImperativeHandle(ref, () => ({
    setSeatModalVisible,
    handleOpenStripePage,
  }));

  const handleOpenStripePage = async (_priceData: IPriceData) => {
    // debugger;
    if (!_priceData?.id) return;
    const res = await fetchCreateOrder(_priceData);
    openWebPage(res.url, '_blank');
    return res;
  };

  const { queryOrgList, orgList } = useOrgStore((s) => ({
    queryOrgList: s.queryOrgList,
    orgList: s.orgList,
  }));

  const { accountInfo } = useUserStore((s) => ({
    accountInfo: s.curUser?.email || s.curUser?.displayName,
  }));
  const language = useGlobalStore((s) => s.baseSetting.language);

  const teamOrgList = useMemo(() => {
    setSelectOrg((orgList || [])[1]?.id);
    return (orgList || [])?.filter((o) => o.type === OrganizationType.TEAM);
  }, [orgList]);

  const handleOrgChange = (value) => {
    setSelectOrg(value);
  };

  const handleSeatChange = (v) => {
    if (v && v > 0) {
      setSeatNumber(v);
    }
  };

  const handleInviteCodeChange = async (code) => {
    setInviteCode(code);
  };

  // Confirm account information.
  const confirmAccount = () => {
    return (
      <Flex justify="space-between" align="center">
        <div>
          <Flex gap={4} align="center">
            {i18n('price.text.confirmAccountInfo')}
            {/* <Tooltip title={i18n('price.payblock.team.select.tips')}>
              <IconfontSvg code="icon-question-mark-circle" size={14} />
            </Tooltip> */}
          </Flex>
          <div className={styles.topSubTitle}>{i18n('price.text.confirmAccountInfoTips')}</div>
        </div>
        <Input disabled style={{ width: '200px' }} value={accountInfo} />
      </Flex>
    );
  };

  /**
   * Render the organization selector.
   */
  const renderOrgSelect = () => {
    return (
      <Flex justify="space-between" align="center">
        <div>
          <Flex gap={4} align="center">
            {i18n('price.payblock.team.select.title')}
            <Tooltip title={i18n('price.payblock.team.select.tips')}>
              <IconfontSvg code="icon-question-mark-circle" size={14} />
            </Tooltip>
          </Flex>
          <div className={styles.topSubTitle}>{i18n('price.payblock.team.select.placeholder')}</div>
        </div>
        <Select
          style={{ width: '200px' }}
          value={selectOrg}
          options={(teamOrgList || []).map((o) => ({
            label: o.name,
            value: o.id,
          }))}
          onChange={handleOrgChange}
        />
      </Flex>
    );
  };

  const renderSeatNumber = () => {
    return (
      <Flex justify="space-between" align="center">
        <div>
          <Flex gap={4} align="center">
            {i18n('price.payblock.seat.title')}
            <Tooltip title={i18n('price.payblock.seat.tips')}>
              <IconfontSvg code="icon-question-mark-circle" size={14} />
            </Tooltip>
          </Flex>
          <div className={styles.topSubTitle}>{i18n('price.payblock.seat.placeholder1')}</div>
        </div>
        <InputNumber
          min={MIN_TEAM_USER_COUNT}
          max={MAX_TEAM_USER_COUNT}
          value={seatNumber}
          onChange={handleSeatChange}
        />
      </Flex>
    );
  };

  // Render the invitation code.
  const renderInviteCode = () => {
    return (
      <>
        <div className={styles.inviteCode}>
          <span>{i18n('invite.price.inviteCode')}</span>
          <Input
            size="small"
            value={inviteCode}
            maxLength={6}
            onChange={(e) => handleInviteCodeChange(e.target.value)}
          />
        </div>
      </>
    );
  };

  useEffect(() => {
    if (!seatModalVisible) {
      return;
    }
    createOrder();
  // Recreate the order when the organization, invitation code, or seat count changes.
  }, [seatModalVisible, selectOrg, seatNumber, inviteCode]);

  const fetchCreateOrder = async (_priceData: IPriceData) => {
    const res = await pricingServices.createOrder({
      id: _priceData.id!,
      paymentMethod: PayType.Stripe,
      seats: _priceData.type === 'team' ? seatNumber : 1,
      organizationId: _priceData.type === 'team' ? selectOrg : userOrgId,
      invitationCode: inviteCode,
      freeTrial: _priceData.freeTrial,
      language,
    });
    return res;
  };

  const createOrder = async () => {
    if (!priceData?.id) return;
    if (priceData.type === 'team' && !selectOrg) {
      queryOrgList({ needCreateOrg: true });
      return;
    }
    setCreateOrderLoading(true);
    try {
      const res = await fetchCreateOrder(priceData);
      setPayDetail(res);
    } catch {
      setCreateOrderLoading(false);
    } finally {
      setCreateOrderLoading(false);
    }
  };

  const continueToBilling = () => {
    if (!payDetail?.url) return;
    openWebPage(payDetail.url, '_blank');
  };

  return (
    <Modal
      className={styles.modal}
      open={!!seatModalVisible}
      centered
      maxHeight={'80vh'}
      width={'684px'}
      footer={null}
      padding={0}
      onCancel={() => {
        setSeatModalVisible(false);
      }}
      maskClosable={false}
      title={null}
      headerBorder={false}
      destroyOnClose
    >
      <div className={styles.modalContent}>
        <div className={styles.modalHeader}>
          <div className={styles.modalEyebrow}>{priceData.type === 'team' ? 'Team' : 'Personal'}</div>
          <div className={styles.modalTitle}>
            {priceData.type === 'team' ? i18n('price.text.confirmTeamInfo') : i18n('price.text.confirmAccount')}
          </div>
        </div>
        <Flex vertical className={styles.payBlockTop} gap={20}>
          {confirmAccount()}
          {priceData.type === 'team' && (
            <>
              {renderOrgSelect()}
              {renderSeatNumber()}
            </>
          )}
        </Flex>
        <div className={styles.bottom}>
          <div className={styles.bottomLeft}>
            <div className={styles.money}>{`USD $${formatPrice(payDetail?.payAmount)} per ${
              payDetail?.subscriptionType?.toLowerCase() || 'year'
            } total`}</div>
            {priceData.type === 'team' && renderInviteCode()}
          </div>
          <Button
            className={styles.button}
            type="primary"
            shape="round"
            loading={createOrderLoading}
            onClick={continueToBilling}
          >
            {i18n('price.button.continue')}
          </Button>
        </div>
      </div>
    </Modal>
  );
});

export default memo(TeamSeatModal);

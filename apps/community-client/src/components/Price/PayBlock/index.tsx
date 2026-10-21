import { useEffect, useMemo, useState } from 'react';
import { Button, DatePicker, Flex, Input, Segmented, Tooltip } from 'antd';
import { SubscriptionType } from '..';
import { useStyles } from './style';
import { InputNumber, IconfontSvg, LoadingGracile, Select } from '@chat2db/ui';
import { PayType } from '@/constants/pricing';
import i18n from '@/i18n';
import { formatCurrency, formatPrice } from '@/utils/price';
import { InviteCodeStatus } from '../PriceMain';
import { useGlobalStore } from '@/store/global';
import invitationService from '@/service/invitation';
import { useOrgStore } from '@/store/organization';
import { CreateOrderResponse, ProductDetail } from '@/typings/pricing';
import { OrganizationType } from '@/typings/enterprise/organization';
import orgService from '@/service/enterprise/organization';
import dayjs from 'dayjs';
import { useUserStore } from '@/store/user';
import { openWebPage } from '@/utils/url';

import weekday from 'dayjs/plugin/weekday';
import localeData from 'dayjs/plugin/localeData';

dayjs.extend(weekday);
dayjs.extend(localeData);

export interface BuyPlanParams {
  paymentMethod: PayType;
  organizationId: number;
  subStartTime?: number;
  invitationCode?: string;
  seats?: number;
}

interface IProps {
  className?: string;
  tabType: SubscriptionType;
  curPricingCard: ProductDetail;
  payUrl?: string;
  curOrderInfo?: CreateOrderResponse;
  invitationCode?: string;
  seats?: number;
  buyPlan: (props: BuyPlanParams) => void;
}

const MIN_LOCAL_USER_COUNT = 1;
const MIN_TEAM_USER_COUNT = 5;
const MAX_TEAM_USER_COUNT = 5 * 1000;

const PayBlock = ({
  tabType,
  curPricingCard,
  payUrl,
  curOrderInfo,
  buyPlan,
  invitationCode: _invitationCode,
  seats: _seats,
}: IProps) => {
  const { styles } = useStyles();

  const [payType, setPayType] = useState<PayType>(PayType.Wechat);
  const [selectOrg, setSelectOrg] = useState<number>();
  const [seatNumber, setSeatNumber] = useState<number>(
    tabType === SubscriptionType.Offline ? MIN_LOCAL_USER_COUNT : MIN_TEAM_USER_COUNT,
  );
  /** End of the previous period; null means there is no subscription. */
  const [endTime, setEndTime] = useState<number>();
  /** Subscription start time. */
  const [subStartTime, setSubStartTime] = useState<number | null>(null);
  const [inviteCode, setInviteCode] = useState<string>('');
  const [inviteCodeStatus, setInviteCodeStatus] = useState<InviteCodeStatus>(InviteCodeStatus.None);
  const rightInviteCode = useMemo(
    () => (inviteCodeStatus === InviteCodeStatus.Valid ? inviteCode : undefined),
    [inviteCodeStatus, inviteCode],
  );
  const [creatOrderLoading, setCreateOrderLoading] = useState<boolean>(false);

  const subscriptType = useUserStore((s) => s.subscriptType);

  const { queryOrgList, orgList, curOrg } = useOrgStore((s) => ({
    queryOrgList: s.queryOrgList,
    orgList: s.orgList,
    curOrg: s.curOrg,
  }));

  const { appUrlConfig } = useGlobalStore((s) => ({
    appUrlConfig: s.appUrlConfig,
  }));

  useEffect(() => {
    if (_invitationCode) {
      handleInviteCodeChange(_invitationCode);
    }
  }, [_invitationCode, _seats]);

  useEffect(() => {
    queryOrgList({ needCreateOrg: true });
  }, []);

  useEffect(() => {
    if (selectOrg) {
      querySubscriptionEndTime();
    }
  }, [selectOrg]);

  // Team organization list.
  const teamOrgList = useMemo(() => {
    return (orgList || [])?.filter((o) => o.type === OrganizationType.TEAM);
  }, [orgList]);

  const showSubStartTime = useMemo(() => {
    if ([SubscriptionType.PersonalUpdate, SubscriptionType.Offline].includes(tabType)) return false;
    /**
     * Hide when endTime or subStartTime is null because there is no subscription.
     */
    if (!endTime || !subStartTime) return false;

    return true;
  }, [subStartTime, endTime, tabType]);

  useEffect(() => {
    // Team edition.
    if (tabType === SubscriptionType.TeamUpdate) {
      setSeatNumber(Math.max(Math.min(_seats || 0, MAX_TEAM_USER_COUNT), MIN_TEAM_USER_COUNT));
      // Select the current team in Team edition; otherwise select the first team.
      if (curOrg?.type === OrganizationType.TEAM) {
        setSelectOrg(curOrg?.id);
      } else {
        setSelectOrg(teamOrgList[0]?.id);
      }
    }
    // Select the personal organization in Personal edition.
    if ([SubscriptionType.PersonalUpdate, SubscriptionType.Offline].includes(tabType)) {
      const personalOrg = orgList?.find((o) => o.type === OrganizationType.PERSONAL);
      setSelectOrg(personalOrg?.id);
      setSeatNumber(1);
    }
  }, [tabType, curOrg, teamOrgList, _seats]);

  useEffect(() => {
    createOrder();
    // Recreate the order when payment method, organization, start time, invitation code, or seat count changes.
  }, [payType, selectOrg, rightInviteCode, seatNumber, subStartTime, curPricingCard, showSubStartTime]);

  const createOrder = async () => {
    if (!curPricingCard || !payType || !selectOrg) return;

    setCreateOrderLoading(true);
    try {
      const params: any = {
        paymentMethod: payType,
        organizationId: selectOrg,
        invitationCode: rightInviteCode,
        seats: seatNumber,
      };

      if (showSubStartTime) {
        params.subStartTime = subStartTime || dayjs().valueOf();
      } else {
        delete params.subStartTime;
      }

      await buyPlan(params);
      setCreateOrderLoading(false);
    } catch {
      setCreateOrderLoading(false);
    }
  };

  const segmentedOptions = useMemo(() => {
    const _segmentedOptions: Array<{ label: string; value: PayType }> = [];
    if (curPricingCard?.paymentMethods?.includes(PayType.Stripe)) {
      setPayType(PayType.Stripe);
      return [];
    }

    if (curPricingCard?.paymentMethods?.includes(PayType.Wechat)) {
      _segmentedOptions.push({
        label: i18n('price.text.wechat'),
        value: PayType.Wechat,
      });
    }
    if (curPricingCard?.paymentMethods?.includes(PayType.Alipay)) {
      _segmentedOptions.push({
        label: i18n('price.text.alipay'),
        value: PayType.Alipay,
      });
    }

    setPayType(_segmentedOptions[0]?.value);
    return _segmentedOptions;
  }, [curPricingCard]);

  const querySubscriptionEndTime = async () => {
    if (!selectOrg) return;

    const res = await orgService.querySubscriptionEndTime({ organizationId: selectOrg });

    setEndTime(res);
    if (subscriptType === SubscriptionType.TeamAddSeat) {
      // Use the current time when adding seats.
      setSubStartTime(dayjs().valueOf());
    } else if (subscriptType === SubscriptionType.TeamUpdate) {
      // Use the expiration time when renewing.
      setSubStartTime(res);
    }
  };

  const handleChangePaymentMethod = (value) => {
    setPayType(value);
  };

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
    if (code.length !== 6) {
      setInviteCodeStatus(InviteCodeStatus.None);
      return;
    }

    const res = await invitationService.getInvitationCodeExit({ code });
    if (res) {
      setInviteCodeStatus(InviteCodeStatus.Valid);
    } else {
      setInviteCodeStatus(InviteCodeStatus.Invalid);
    }
  };

  /**
   * Render the payment block header.
   */
  const renderPayBlockTop = () => {
    if (tabType === SubscriptionType.PersonalUpdate) return;
    return (
      <Flex vertical className={styles.payBlockTop} gap={20}>
        {renderOrgSelect()}
        {renderSeatNumber()}
        {renderSubStartTime()}
      </Flex>
    );
  };

  /**
   * Render the organization selector.
   */
  const renderOrgSelect = () => {
    // if (subscriptType !== SubscriptionType.TeamUpdate) return;
    if (tabType !== SubscriptionType.TeamUpdate || subscriptType === SubscriptionType.TeamAddSeat) return;
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
          style={{ width: '270px' }}
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

  /**
   * Render the seat selector.
   */
  const renderSeatNumber = () => {
    const isAddSeat = subscriptType === SubscriptionType.TeamAddSeat;
    const isTeamSeat = tabType === SubscriptionType.TeamUpdate;
    const isLocale = tabType === SubscriptionType.Offline;
    return (
      <Flex justify="space-between" align="center">
        <div>
          <Flex gap={4} align="center">
            {i18n('price.payblock.seat.title')}
            {!isLocale && (
              <Tooltip title={i18n('price.payblock.seat.tips')}>
                <IconfontSvg code="icon-question-mark-circle" size={14} />
              </Tooltip>
            )}
          </Flex>
          <div className={styles.topSubTitle}>
            {isAddSeat && i18n('price.payblock.seat.placeholder1')}
            {isTeamSeat && i18n('price.payblock.seat.placeholder2')}
            {isLocale && i18n('price.payblock.seat.placeholder3')}
          </div>
        </div>
        <InputNumber
          min={isLocale ? 1 : MIN_TEAM_USER_COUNT}
          max={MAX_TEAM_USER_COUNT}
          value={seatNumber}
          onChange={handleSeatChange}
        />
      </Flex>
    );
  };

  /**
   * Render the time selector.
   */
  const renderSubStartTime = () => {
    if (!showSubStartTime) return null;

    return (
      <Flex justify="space-between" align="center">
        <div>{i18n('price.payblock.starttime.title')}</div>
        <DatePicker
          style={{ width: '270px' }}
          value={dayjs(subStartTime)}
          allowClear={false}
          disabledDate={(current) => {
            // Current time <= T <= previous order end time.
            return current < dayjs() || current > dayjs(endTime);
          }}
          onChange={(d) => {
            setSubStartTime(d.valueOf());
          }}
        />
      </Flex>
    );
  };

  const renderPayMain = () => {
    // stripe
    if (payType === PayType.Stripe) {
      return (
        <Flex vertical align="center" gap={16} style={{ marginTop: '60px' }}>
          <Button
            className={styles.payButton}
            type="primary"
            loading={creatOrderLoading}
            onClick={() => {
              payUrl && openWebPage(payUrl);
            }}
          >
            <span>{i18n('price.button.payNow')}</span>
            <span>
              {formatCurrency(curOrderInfo?.currency)}
              {formatPrice(curOrderInfo?.payAmount)}
            </span>
          </Button>
          <div className={styles.privacyAgreement}>
            {i18n('price.text.agreement')}
            <a href={appUrlConfig.SERVICE_AGREEMENT} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.termsOfService')}》
            </a>
            <a href={appUrlConfig.PRIVACY_POLICY} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.privacyPolicy')}》
            </a>
            <a href={appUrlConfig.MEMBER_AGREEMENT} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.memberAgreement')}》
            </a>
          </div>
        </Flex>
      );
    }

    // WeChat Pay and Alipay.
    return (
      <Flex align="stretch">
        <div className={styles.payBlockLeft}>
          <div className={styles.payQRCode}>
            {payUrl ? (
              <>
                <img src={payUrl} alt="" />
                {payType && (
                  <div className={styles.payTypeBox}>
                    <IconfontSvg size={26} code={`icon-colourful-${payType}`} />
                  </div>
                )}
              </>
            ) : (
              <LoadingGracile />
            )}
          </div>
          {segmentedOptions.length > 1 && (
            <Segmented
              className={styles.segmentedBox}
              value={payType}
              onChange={(value) => handleChangePaymentMethod(value)}
              options={segmentedOptions}
            />
          )}
        </div>
        <div className={styles.payBlockRight}>
          <div className={styles.payBlockRightTitle}>{i18n('price.label.amountPayable')}</div>
          <div className={styles.pricePaidWrapper}>
            {/* Discounted price. */}
            <div className={styles.priceActuallyPaid}>
              {formatCurrency(curOrderInfo?.currency)}
              {formatPrice(curOrderInfo?.payAmount)}
            </div>
            {/* Original price. */}
            {curOrderInfo?.amount !== curOrderInfo?.payAmount && (
              <div className={styles.priceOriginPaid}>
                {formatCurrency(curOrderInfo?.currency)}
                {formatPrice(curOrderInfo?.amount)}
              </div>
            )}
          </div>
          <div className={styles.inviteCode}>
            <span>{i18n('invite.price.inviteCode')}</span>
            <Input
              size="small"
              value={inviteCode}
              maxLength={6}
              onChange={(e) => handleInviteCodeChange(e.target.value)}
            />
            {inviteCodeStatus === InviteCodeStatus.Valid ? (
              <IconfontSvg code="icon-danse" size={16} />
            ) : inviteCodeStatus === InviteCodeStatus.Invalid ? (
              <div className="error">{i18n('invite.price.inviteCode.invalid')}</div>
            ) : (
              ''
            )}
          </div>
          <div className={styles.privacyAgreement}>
            {i18n('price.text.agreement')}
            <a href={appUrlConfig.SERVICE_AGREEMENT} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.termsOfService')}》
            </a>
            <a href={appUrlConfig.PRIVACY_POLICY} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.privacyPolicy')}》
            </a>
            <a href={appUrlConfig.MEMBER_AGREEMENT} target="_blank" rel="noopener noreferrer">
              《{i18n('login.text.memberAgreement')}》
            </a>
          </div>
        </div>
      </Flex>
    );
  };

  return (
    <div className={styles.payBlock}>
      {renderPayBlockTop()}
      {renderPayMain()}
    </div>
  );
};

export default PayBlock;

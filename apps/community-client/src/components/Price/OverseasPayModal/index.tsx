import { useEffect, useMemo, useState } from 'react';
import { Skeleton } from 'antd';
import { Modal } from '@chat2db/ui';
import { useGlobalStore } from '@/store/global';
import { useUserStore } from '@/store/user';
import { PayStatus, SubscriptionType } from '@/constants/pricing';
import { useStyles } from './style';
import i18n from '@/i18n';
import { IPriceData, ISubscriptionType } from '@/typings/pricing';
import { formatPrice, formatCurrency } from '@/utils/price';
import pricingServices from '@/service/pricing';
import PricingCard from './PricingCard';
import { ErrorCode } from '@/constants';
import { collectProductDescriptions, normalizeProductDetail } from '../utils';
import { PRICING_AUTO_POPUP_KEY } from '@/layouts/init/initQuery';

const OverseasPayModal = () => {
  const { styles, cx } = useStyles();
  const { queryCurUser, pricingModalStatus, setPricingModalStatus, freeTrial } = useUserStore((s) => ({
    setPricingModalStatus: s.setPricingModalStatus,
    pricingModalStatus: s.pricingModalStatus,
    queryCurUser: s.queryCurUser,
    freeTrial: s.pricingModalStatus === ErrorCode.FreeTrialUSageLimit || false,
  }));

  const localProductList: IPriceData[] = useMemo(
    () => [
      {
        type: 'personal',
        title: i18n('price.text.chat2dbPro.title'),
        subtitle: i18n('price.text.chat2dbPro.subTitle'),
        describe: '',
        price: {
          [SubscriptionType.monthly]: {
            id: 1,
            price: '0',
            priceSuffix: freeTrial
              ? `/${i18n('price.text.freePriceSuffix')}`
              : `/${i18n('price.text.monthly').toLowerCase()}`,
            thenPriceSuffix: freeTrial
              ? `${i18n('price.text.thenPriceSuffix')} {price} /${i18n('price.text.monthly').toLowerCase()}`
              : null,
          },
          [SubscriptionType.yearly]: {
            id: 2,
            price: '0',
              // Show the monthly amount for yearly plans as well.
            priceSuffix: freeTrial
              ? `/${i18n('price.text.freePriceSuffix')}`
              : `/${i18n('price.text.monthly').toLowerCase()}`,
            thenPriceSuffix: freeTrial
              ? `${i18n('price.text.thenPriceSuffix')} {price} /${i18n('price.text.yearly').toLowerCase()}`
              : null,
          },
          [SubscriptionType.forever]: {
            id: 3,
            price: '0',
            priceSuffix: `/${i18n('price.text.forever').toLowerCase()}`,
          },
        },
        featuresTitle: i18n('price.text.professionalPower.title'),
        features: [
          {
            icon: 'icon-1',
            label: i18n('price.text.professionalPower1'),
          },
          {
            icon: 'icon-2',
            label: i18n('price.text.professionalPower2'),
          },
          {
            icon: 'icon-3',
            label: i18n('price.text.professionalPower3'),
          },
          {
            icon: 'icon-4',
            label: i18n('price.text.professionalPower4'),
          },
          {
            icon: 'icon-5',
            label: i18n('price.text.professionalPower5'),
          },
          {
            icon: 'icon-6',
            label: i18n('price.text.professionalPower6'),
          },
          {
            icon: 'icon-7',
            label: i18n('price.text.professionalPower7'),
          },
        ],
        freeTrial: freeTrial,
        buyButtonText: !freeTrial ? i18n('price.text.updateToPro') : i18n('price.text.startFreeButton'),
      },
      {
        type: 'team',
        title: i18n('price.text.chat2dbProTeam.title'),
        subtitle: i18n('price.text.chat2dbProTeam.subTitle'),
        describe: '',
        mostPopular: true,
        price: {
          [SubscriptionType.monthly]: {
            id: 21,
            price: '0',
            priceSuffix: `${i18n('price.text.perUser')}/${i18n('price.text.monthly').toLowerCase()}`,
          },
          [SubscriptionType.yearly]: {
            id: 22,
            price: '0',
              // Show the monthly amount for yearly plans as well.
            priceSuffix: `${i18n('price.text.perUser')}/${i18n('price.text.monthly').toLowerCase()}`,
          },
          [SubscriptionType.forever]: {
            id: 23,
            price: '0',
            priceSuffix: `${i18n('price.text.perUser')}/${i18n('price.text.forever').toLowerCase()}`,
          },
        },
        featuresTitle: i18n('price.text.teamPower.title'),
        features: [
          {
            icon: 'icon-1',
            label: i18n('price.text.teamPower1'),
          },
          {
            icon: 'icon-2',
            label: i18n('price.text.teamPower2'),
          },
          {
            icon: 'icon-3',
            label: i18n('price.text.teamPower3'),
          },
          {
            icon: 'icon-4',
            label: i18n('price.text.teamPower4'),
          },
          {
            icon: 'icon-5',
            label: i18n('price.text.teamPower5'),
          },
          {
            icon: 'icon-6',
            label: i18n('price.text.teamPower6'),
          },
          {
            icon: 'icon-7',
            label: i18n('price.text.teamPower7'),
          },
        ],
        buyButtonText: i18n('price.text.updateToTeam'),
      },
    ],
    [freeTrial],
  );

  const { payStatus, appConfig, baseSetting } = useGlobalStore((s) => ({
    payStatus: s.payStatus,
    appConfig: s.appConfig,
    baseSetting: s.baseSetting,
  }));

  useEffect(() => {
    if (payStatus === PayStatus.PAY_SUCCESS) {
      queryCurUser();
      setPricingModalStatus(false);
    }
  }, [payStatus]);

  const frequencies: ISubscriptionType[] = useMemo(() => {
    if (appConfig.isCN) {
      return [
        {
          value: SubscriptionType.yearly,
          label: i18n('price.text.yearly'),
          priceSuffix: `/${i18n('price.text.monthly').toLowerCase()}`,
        },
        {
          value: SubscriptionType.forever,
          label: i18n('price.text.forever'),
          priceSuffix: `/${i18n('price.text.monthly').toLowerCase()}`,
        },
      ];
    } else {
      return [
        {
          value: SubscriptionType.monthly,
          label: i18n('price.text.monthly'),
          priceSuffix: `/${i18n('price.text.monthly').toLowerCase()}`,
        },
        {
          value: SubscriptionType.yearly,
          label: i18n('price.text.yearly'),
          priceSuffix: `/${i18n('price.text.monthly').toLowerCase()}`,
          badge: i18n('price.text.saveDiscount'),
        },
      ];
    }
  }, [appConfig.isCN]);

  const defaultTab = appConfig.isCN
    ? frequencies[frequencies.length - 1]
    : frequencies.find((frequency) => frequency.value === SubscriptionType.yearly) || frequencies[0];
  const [activeTab, setActiveTab] = useState<ISubscriptionType>(defaultTab);
  const [productList, setProductList] = useState<any>();
  const [loading, setLoading] = useState(false);

  const getLocalTemplate = (versionType: string) => {
    if (versionType === 'PRO') return localProductList[0];
    if (versionType === 'PRO2') return localProductList[1];
    return undefined;
  };

  const getProductList = (subscriptionTypes) => {
    setLoading(true);
    pricingServices.getProductList({ subscriptionTypes, language: baseSetting.language }).then((r) => {
      if (!r?.length) {
        setLoading(false);
        return;
      }

      const middleIndex = Math.floor(r.length / 2);
      const newProductList = r.map((item, index) => {
        const normalizedProduct = normalizeProductDetail(item, baseSetting.language);
        const isYearly = activeTab.value === SubscriptionType.yearly;
        const effectivePrice = normalizedProduct.previousTwoMonthsPrice ?? normalizedProduct.price;
        const price = isYearly ? Number(effectivePrice) / 12 : effectivePrice;
        const yearlyPrice = isYearly ? effectivePrice : undefined;

        const localTemplate = getLocalTemplate(normalizedProduct.versionType);
        const descriptions = collectProductDescriptions(normalizedProduct);
        const priceSuffix = localTemplate?.price?.[activeTab.value]?.priceSuffix
          || `/${activeTab.label.toLowerCase()}`;

        let thenPriceSuffix: string | null | undefined = undefined;
        const thenTemplate = localTemplate?.price?.[activeTab.value]?.thenPriceSuffix;
        if (thenTemplate) {
          thenPriceSuffix = thenTemplate.replace(
            /\{price\}/g,
            `${formatCurrency(normalizedProduct.currency)}${formatPrice(yearlyPrice || normalizedProduct.price)}`,
          );
        } else if (normalizedProduct.previousTwoMonthsPrice) {
          const promoDisplay = `${formatCurrency(normalizedProduct.currency)}${formatPrice(price)}${priceSuffix}`;
          const regularPerUnit = isYearly ? Number(normalizedProduct.price) / 12 : normalizedProduct.price;
          const regularDisplay =
            `${formatCurrency(normalizedProduct.currency)}${formatPrice(regularPerUnit)}${priceSuffix}`;
          thenPriceSuffix = i18n('price.text.previousTwoMonthsHint', promoDisplay, regularDisplay);
        }

        const isPersonalType = normalizedProduct.versionType === 'PRO';
        let curPrice = `${formatCurrency(normalizedProduct.currency)}${
          freeTrial && isPersonalType ? '0' : formatPrice(price)
        }`;
        let displayPriceSuffix: string = priceSuffix;

        if (normalizedProduct.freeTrialDays) {
          curPrice = i18n('price.text.freeTrialDaysLabel', String(normalizedProduct.freeTrialDays));
          displayPriceSuffix = '';
          const perMonthPrice = isYearly ? Number(normalizedProduct.price) / 12 : normalizedProduct.price;
          thenPriceSuffix = i18n(
            'price.text.freeTrialBilledYearlyHint',
            `${formatCurrency(normalizedProduct.currency)}${formatPrice(perMonthPrice)}`,
          );
        }

        const productTitle = normalizedProduct.title || localTemplate?.title || '';

        return {
          type: normalizedProduct.versionType === 'PRO2' ? 'team' : 'personal',
          title: productTitle,
          subtitle: normalizedProduct.shortDescription || localTemplate?.subtitle || productTitle,
          describe: '',
          mostPopular: r.length >= 3 ? index === middleIndex : (localTemplate?.mostPopular || false),
          featuresTitle: '',
          features: descriptions.length
            ? descriptions.map((label, featureIndex) => ({ icon: `icon-${featureIndex + 1}`, label }))
            : localTemplate?.features || [],
          buyButtonText: normalizedProduct.buttonText || `Upgrade to ${productTitle}`,
          freeTrial: isPersonalType ? freeTrial : false,
          id: normalizedProduct.id,
          curPrice,
          priceSuffix: displayPriceSuffix,
          thenPriceSuffix,
        } as IPriceData;
      });

      setProductList(newProductList);
      setLoading(false);
    });
  };

  useEffect(() => {
    if (pricingModalStatus) {
      getProductList(activeTab.value);
    }
  }, [pricingModalStatus, activeTab, baseSetting.language]);

  return (
    <>
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
          localStorage.setItem(PRICING_AUTO_POPUP_KEY, String(Date.now()));
        }}
        maskClosable={false}
        destroyOnClose
      >
        <div className={styles.modalContent}>
          <div className={styles.segmentedBox}>
            {freeTrial && (
              <>
                <div className={styles.segmentedTitle}>{i18n('price.text.startFreeTitle')}</div>
                <div className={styles.segmentedSubTitle}>{i18n('price.text.startFreeSubTitle')}</div>
              </>
            )}

            <div className={styles.segmentedList}>
              {frequencies.map((f) => {
                return (
                  <div
                    key={f.value}
                    className={cx({ [styles.segmentedItemActive]: f.value === activeTab.value }, styles.segmentedItem)}
                    onClick={() => {
                      setActiveTab(f);
                    }}
                  >
                    <span>{f.label}</span>
                    {f.badge && <span className={styles.segmentedBadge}>{f.badge}</span>}
                  </div>
                );
              })}
            </div>
          </div>
          <div className={styles.pricingCardList}>
            {loading || !productList
              ? [0, 1, 2].map((i) => (
                  <div key={i} className={styles.skeletonCard}>
                    <Skeleton active paragraph={{ rows: 4, width: ['40%', '60%', '100%', '80%'] }} title={{ width: '50%' }} />
                    <Skeleton.Button active block style={{ height: 40, borderRadius: 999, marginTop: 16 }} />
                    <Skeleton active paragraph={{ rows: 5, width: ['90%', '85%', '80%', '75%', '70%'] }} title={false} style={{ marginTop: 16 }} />
                  </div>
                ))
              : productList.map((item, index) => <PricingCard key={index} priceData={item} />)}
          </div>
        </div>
      </Modal>
    </>
  );
};
export default OverseasPayModal;

import { PayType, PayStatus, SubscriptionType } from '@/constants/pricing';

export type LocalizedFeatureMap = Record<string, string[]>;

export interface CreateOrderResponse {
  payType: PayType;
  orderId: string;
  url: string;
  amount: number;
  payAmount: number;
  discountedAmount?: number;
  currency: string;
  subscriptionType?: string;
  qrCodeUrl?: string;
  paymentUrl?: string;
}

export interface OrderDetail {
  amount: string;
  payType: PayType;
  url: string;
  status: PayStatus;
  orderDate: string;
}

export interface GoodsPriceDetail {
  [SubscriptionType.monthly]: {
    id: number;
    price: string;
    originalPrice?: string;
    priceSuffix: string;
    thenPriceSuffix?: string | null;
  };
  [SubscriptionType.yearly]: {
    id: number;
    price: string;
    originalPrice?: string;
    discountPercentage?: string;
    priceSuffix: string;
    thenPriceSuffix?: string | null;
  };
  [SubscriptionType.forever]: {
    id: number;
    price: string;
    originalPrice?: string;
    priceSuffix: string;
    thenPriceSuffix?: string | null;
  };
}

export interface IPriceData {
  id?: number;
  title: string;
  type: 'team' | 'personal';
  subtitle: string;
  describe: string;
  mostPopular?: boolean;
  price?: GoodsPriceDetail;
  curPrice?: string;
  priceSuffix?: string;
  featuresTitle: string;
  thenPriceSuffix?: string | null;
  freeTrial?: boolean; // Whether to try it out
  features: {
    icon: string;
    label: string;
  }[];
  buyButtonText: string;
}

export interface ProductDetailVO {
  id: number;
  attach?: string;
  title: string;
  price: string; // price
  // Prefer this recent price when present; mutually exclusive with freeTrialDays.
  previousTwoMonthsPrice?: string;
  // Display the free-trial label first; mutually exclusive with previousTwoMonthsPrice.
  freeTrialDays?: number;
  yearlyPrice?: string; // annual price
  originalPrice: string; // original price
  currency: string; // Currency
  versionType: string; // version type
  productType: string; // product type
  subscriptionType: string; // Subscription type
  paymentMethods: PayType[];
  freeTrial?: boolean; // Whether to try it out
  tag: string; // label
  description: string; // Description
  description1: string; // Description
  description2: string; // Description
  descriptionList?: string[];
  shortDescription?: string;
  buttonText?: string;
  feature?: LocalizedFeatureMap | string;
  localizedFeatures?: string[];
}

export interface ProductDetail {
  id: number;
  attach?: string;
  title: string;
  invitedDiscountPrice?: string; // Invite discount price
  price: string; // price
  // Prefer this recent price when present; mutually exclusive with freeTrialDays.
  previousTwoMonthsPrice?: string;
  // Display the free-trial label first; mutually exclusive with previousTwoMonthsPrice.
  freeTrialDays?: number;
  originalPrice?: string; // original price
  currency: string; // Currency
  currencySymbol: string; // currency symbol
  versionType: string; // version type
  productType: string; // product type
  subscriptionType: string; // Subscription type
  paymentMethods: PayType[];

  tag: string; // label
  description: string; // Description
  description1: string; // Description
  description2: string; // Description
  descriptionList?: string[];
  shortDescription?: string;
  buttonText?: string;
  feature?: LocalizedFeatureMap | string;
  localizedFeatures?: string[];
}

export interface ISubscriptionType {
  value: SubscriptionType;
  label: string;
  priceSuffix: string;
  badge?: string;
}

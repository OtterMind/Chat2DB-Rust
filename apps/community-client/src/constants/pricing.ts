export enum PayType {
  Stripe = 'stripe',
  Wechat = 'wechat',
  Alipay = 'alipay',
}

export enum PayStatus {
  CREATE = 'CREATE',
  PAY_SUCCESS = 'PAY_SUCCESS',
  CANCEL = 'CANCEL',
}

export enum SubscriptionType {
  monthly = 'MONTH',
  yearly = 'YEAR',
  forever = 'FOREVER',
}

import { LoginDetailType } from './oauth';
import { RoleCodeType } from './role';
import { IOrganizationVO } from './organization';
import { SubscriptionType, SubscriptionStatus } from '@/constants/user';

/**
 * Data information UserVO
 */
export interface IUserVO {
  /**
   * Primary key
   */
  id: number;

  /**
   * Display name
   */
  displayName: string;

  /**
   * role code
   */
  roleCode: RoleCodeType;

  /**
   * Avatar
   */
  avatar: string;

  /**
   * Current organization
   */
  currentOrganization: IOrganizationVO;

  /**
   * Email
   */
  email: string;

  /**
   * Whether VIP
   */
  vip: boolean;

  /**
   * Country
   */
  country: string;

  /**
   * Language
   */
  language: string;

  /**
   * Creation time
   */
  createTime: number;
  /**
   * Whether to activate
   */
  activated: boolean;
  /**
   * Trial start time
   */
  trialStartTime: number;
  /**
   *Pure offline activation
   */
  networkAbandoned: boolean;

  /**
   * Subscription type, such as "Starter", "Pro", "Pro2", etc., returned by the backend interface
   */
  subscriptionType?: string;
}

export interface IUserLoginVO {
  /**
   * Email
   */
  email?: string;
  /**
   * Login type: EMAIL/PHONE
   */
  loginType: LoginDetailType;
  /**
   * Username
   */
  name?: string;
  /**
   * Nickname
   */
  nickName?: string;
  /**
   * Password
   */
  password?: string;
  /**
   *Mobile phone number
   */
  phoneNumber?: string;
  passcode?: string;
}

export interface Subscription {
  id: number;
  userId: number;
  orgId: number;
  type: SubscriptionType;
  price: number;
  currency: string;
  quantity: number; // Product quantity
  status: SubscriptionStatus;
  startTime: number;
  endTime: number;
  createTime: number;
  /** Seats */
  seats: number;
  items: Subscription[];
}

export interface CountryItem {
  code: string;
  name: string;
  appUrl: string;
  current: boolean;
  redirect: boolean;
  gatewayUrl: string;
}

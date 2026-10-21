export enum InvitationStatusCode {
  /** Withdrawing */
  WITHDRAWING = 'WITHDRAWING',
  /** Withdrawn */
  WITHDRAWED = 'WITHDRAWED',
  /** Pending withdrawal */
  WAIT_WITHDRAW = 'WAIT_WITHDRAW',
}

export interface InvitationOrderVO {
  /** Total assets */
  totalAmount: string;
  /** Amount withdrawn */
  withdrawAmount: string;
  /** Amount being withdrawn */
  withdrawingAmount: string;
  /** Amount of cash available */
  canWithdrawAmount: string;
  /** Amount to be withdrawn */
  waitWithdrawAmount: string;
  /** Invitation list data */
  invitationOrderItems: InvitationOrderListItem[];
}

export interface InvitationOrderListItem {
  id: number;
  createTime: number;
  updateTime: number;
  invitationCode: string;
  status: string;
  invitationUserId: number;
  invitationUserDisplayName: string;
  invitationOrderId: number;
  amount: string;
  productName: string;
  productPrice: string;
}

export interface WithdrawIncomeParam {
  name: string;
  cardNumber: string;
  aliPayAccount: string;
  userId: string;
  phoneNum: string;
}

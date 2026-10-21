import i18n from '@/i18n';
import { BooleanType } from '../common';
import { IUserVO } from '@/typings/enterprise/user';

export enum ApprovalStatusType {
  'INVOKED' = 'INVOKED', // Initiated
  'APPROVED' = 'APPROVED', // Approval passed
  'PENDING' = 'PENDING', // Pending approval
  'REVOKED' = 'REVOKED', // Cancel
  'TRANSFERRED' = 'TRANSFERRED', // transfer
  'FINISHED' = 'FINISHED', // Complete
  'REJECTED' = 'REJECTED', // Dismiss
  'ADD_SIGNATURE' = 'ADD_SIGNATURE', // Affidavit
}

export const ApprovalStatusMap = {
  INVOKED: i18n('team.approval.status.invoked'), // Initiated
  APPROVED: i18n('team.approval.status.approved'), //
  PENDING: i18n('team.approval.status.pending'), //
  REVOKED: i18n('team.approval.status.revoked'), //
  TRANSFERRED: i18n('team.approval.status.transferred'), //
  FINISHED: i18n('team.approval.status.finished'), //
  REJECTED: i18n('team.approval.status.reject'), //
  ADD_SIGNATURE: i18n('team.approval.status.addSignature'), //
};

export interface IApprovalProcessVO {
  /**
   * Primary key
   */
  id: number;
  /**
   * Parent ID
   */
  parentId: number;
  /**
   * Root ID
   */
  rootId: number;
  /**
   * Level
   */
  approvalLevel: number;
  /**
   *Organization ID
   */
  organizationId: number;
  /**
   * Application ID
   */
  applyId: number;
  /**
   * Application name
   */
  name: string;
  /**
   *Application description
   */
  description: string;
  /**
   * Applicant ID
   */
  applyUser: IUserVO;

  applyType: string;
  /**
   * Status
   */
  approvalStatus: ApprovalStatusType;
  /**
   * Approval description
   */
  approvalDescription: string;
  /**
   * ID of person to be approved
   */
  awaitingUser: IUserVO;

  approveUser: IUserVO;
  /**
   * Approval user ID
   */
  approveUserId: number;
  /**
   * Transferee ID
   */
  transferUserId: number;
  /**
   * Approver ID
   */
  signatureUserId: number;

  /**
   * End of validity period
   */
  validUntil: number;
  /**
   * Is it permanently valid: Y/N
   */
  noExpire: BooleanType;

  /**
   * Creation time
   */
  createTime: number;
  /**
   * Create user ID
   */
  createUserId: number;
  /**
   * Update time
   */
  modifyTime: number;
  /**
   * Modify user ID
   */
  modifyUserId: number;

  child: IApprovalProcessVO[];
}

export type IApprovalProcesCanModifyVO = {
  id: number;
  approvalStatus: ApprovalStatusType;
  transferUserId?: number;
  signatureUserId?: number;
  approvalDescription?: string;
};

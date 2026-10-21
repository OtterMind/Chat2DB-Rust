export enum GroupStatusType {
  VALID = 'VALID',
  INVALID = 'INVALID',
}

export interface IGroupVO {
  /**
   * Primary key
   */
  id: number;
  /**
   *Organization ID
   */
  organizationId: number;
  /**
   * User group code
   */
  code: string;
  /**
   * User group name
   */
  name: string;

  /**
   * Status VALID/INVALID
   */
  status: GroupStatusType;
  /**
   * Creator user ID
   */
  creatorUserId: number;

  /**
   * Modifier user ID
   */
  modifierUserId: number;

  /**
   * Creation time
   */
  createTime: number;

  /**
   * Update time
   */
  modifyTime: number;
}

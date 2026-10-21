export enum RoleCodeType {
  ADMIN = 'ADMIN',
  OPERATOR = 'OPERATOR',
  OWNER = 'OWNER',
}

export enum RoleStatusType {
  VALID = 'VALID',
  INVALID = 'INVALID',
}
export interface IRoleVO {
  /**
   * Primary key
   */
  id: number;
  /**
   *Organization ID
   */
  organizationId: number;
  /**
   * Role name
   */
  name: string;
  /**
   * role code
   */
  code: string;
  /**
   *Type SYSTEM/CUSTOM
   */
  type: string;
  /**
   * Status VALID/INVALID
   */
  status: RoleStatusType;
  /**
   * Delete ID
   */
  deletedId: number;
  /**
   * Create user ID
   */
  createUserId: number;
  /**
   * Modify user ID
   */
  modifyUserId: number;
  /**
   * Creation time
   */
  createTime: number;

  /**
   *Deletion time
   */
  deleteTime: number;

  /**
   * Update time
   */
  modifyTime: number;
}

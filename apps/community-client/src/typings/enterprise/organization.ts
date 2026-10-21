export enum OrganizationType {
  PERSONAL = 'PERSONAL',
  ENTERPRISE = 'ENTERPRISE',
  TEAM = 'TEAM',
}

export enum OrganizationStatusType {
  VALID = 'VALID',
  INVALID = 'INVALID',
}

export enum OrgUserRoleCode {
  SUPER_ADMIN = 'SUPER_ADMIN',
  ADMIN = 'ADMIN',
  OPERATOR = 'OPERATOR',
}

export interface IOrganizationVO {
  /**
   * Primary key
   */
  id: number;
  /**
   *Organization name
   */
  name: string;
  /**
   *Enterprise type
   */
  type: OrganizationType;
  /**
   * Status
   */
  status: OrganizationStatusType;
  /**
   * Owner ID
   */
  ownerId: number;
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
   * Update time
   */
  modifyTime: number;

  /**
   * The role of the current logged-in person
   */
  roleCodes: OrgUserRoleCode[];

  /** Whether it is VIP */
  vip: boolean;

  /** Area */
  area: string;

  /** Industry */
  industry: string;

  /** role */
  role: string;

  /**
   * Enterprise number
   */
  organizationCode: string;

  /**
   * Corporate avatar
   */
  organizationAvatar: string;

  inviterName?: string;

  /**
   *Number of seats
   */
  seats: number;

  /**
   * Current number of team members
   */
  currentMemberCount: number;
}

export type IUpdateOrganizationVORequest = Pick<IOrganizationVO, 'id'> &
  Partial<Pick<IOrganizationVO, 'name' | 'status' | 'organizationAvatar'>>;

export interface IOrganizationUserVO {
  id: number;
  status: OrganizationStatusType;
  registerSource: string;
  // "registerType": "EMAIL",
  // "token": null,
  createUserId: number;
  modifyUserId: number;
  createTime: number;
  modifyTime: number;
  displayName: string;
  inviteUserId: number;
  registerSourceType: null;
  roleCodes: OrgUserRoleCode[];
}

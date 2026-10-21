import { IUserVO } from './user';
import { BooleanType } from '../common';
import { RoleCodeType } from './role';
import { ApprovalStatusType } from './approval';

export enum PermissionType {
  AUTH = 'auth',
  APPLY = 'apply',
  DATA_MARKET = 'data-market',
}

export enum PermissionTypeName {
  AUTH = '授权列表',
  APPLY = '申请列表',
  DATA_MARKET = '数据集市',
}

/**
 * Authorization permission type
 */
export enum AuthType {
  Admin = 'ADMIN',
  OWNER = 'OWNER',
  OPERATOR = 'OPERATOR',
  SCRIPT = 'SCRIPT',
}

/**
 * Application permission type
 */
export enum ApplyType {
  Admin = 'ADMIN',
  OWNER = 'OWNER',
  OPERATOR = 'OPERATOR',
  SCRIPT = 'SCRIPT',
}

export enum AuthSubjectType {
  USER = 'USER',
  ROLE = 'ROLE',
  GROUP = 'GROUP',
}

export enum permissionCodeType {
  ADMIN = 'ADMIN',
  OWNER = 'OWNER',
}

export enum accessControlPolicyScopeCodeType {
  DML = 'DML',
  DCL_DDL = 'DCLDDL',
  OTHER = 'OTHER',
}

export enum accessControlPolicyCodeType {
  SELECT = 'SELECT',
  UPDATE = 'UPDATE',
  INSERT = 'INSERT',
  DELETE = 'DELETE',
  GRANT = 'GRANT',
  ALTER = 'ALTER',
  DROP = 'DROP',
  CREATE = 'CREATE',
  TRUNCATE = 'TRUNCATE',
  CALL = 'CALL',
  VIEW = 'VIEW',
}

/**
 * Apply for data permission
 */
export interface IAuthDataAccessRequest {
  /**
   *Organization ID
   */

  organizationId: number;

  /**
   *Data source ID
   */
  dataSourceId: number;

  /**
   * name
   */
  name: string;

  /**
   * Whether the entire database is authorized
   */
  isAllSchema: boolean;

  /**
   * Data source name
   */
  databaseName: string;

  /**
   * schema name
   */
  schemaName: string;

  /**
   *Table name
   */
  tableName: string;

  /**
   * Column filter name
   */
  columnNames?: string[];

  /**
   * Row filter conditions
   */
  rowFilter?: string;

  /**
   * Number of rows that can be viewed at a time
   */
  rowCount?: number;

  /**
   * Authorization policy secondary encoding
   */
  policyVOList: string[];

  /**
   * Role code: OPERATOR
   */
  roleCode?: RoleCodeType;

  /**
   * Permission validity period
   */
  validUntil?: number;

  /**
   *User ID
   */
  userId: number;

  /**
   * Is it permanently valid?
   */
  noExpire: string;

  /**
   *Application description
   */
  description: string;
}

export interface IApplyRolePermission {
  id?: number;
  name?: string;
  description?: string;
  organizationId?: number;
  userId?: number | { label: string; value: number };
  dataSourceId?: number | { label: string; value: number };
  roleCode?: permissionCodeType;
  noExpire?: BooleanType;
  validUntil?: number;
}

/**
 * Authorized administrator request parameters
 */
export interface IAuthAdminRequest {
  /**
   *Organization ID
   */
  organizationId: number;

  /**
   * Data source ID
   */
  dataSourceId: number;

  /**
   * Authorization type: ADMIN/OWNER
   */
  authType: AuthType;

  /**
   * Permission subject type: USER/GROUP/ROLE
   */
  authSubjectType: AuthSubjectType;

  /**
   * Permission validity period
   */
  validUntil?: number;

  /**
   * Application content
   */
  name: string;

  /**
   *Application description
   */
  description: string;

  /**
   * Is it permanently valid?
   */
  noExpire: BooleanType;

  /**
   *User ID
   */
  userId: number;
}

export interface IApplyDataAccessRequest {
  /**
   *Organization ID
   */
  organizationId: number;

  /**
   *Data source ID
   */
  dataSourceId: number;

  /**
   * Application name
   */
  name: string;

  /**
   * Whether the entire database is authorized
   */
  isAllSchema: boolean;

  /**
   * Data source name
   */
  databaseName: string;

  /**
   * schema name
   */
  schemaName: string;

  /**
   *Table name
   */
  tableName: string;

  /**
   * Column filter name, multiple separated by commas
   */
  columnNames: string[];

  /**
   * Row filter conditions
   */
  rowFilter?: string;

  /**
   * Number of rows that can be viewed at a time
   */
  rowCount?: number;

  /**
   * Second level authorization code
   */
  policyVOList: string[];

  /**
   * Is it permanently valid?
   */
  noExpire: BooleanType;

  validUntil?: number;
  /**
   *Application description
   */
  description: string;
}

export interface IApplyDataPermission {
  name: string;
  description: string;
  userId: number;

  /**
   *Organization ID
   */
  organizationId: number;
  /**
   *Data source ID
   */
  dataSourceId: number;

  /**
   * Whether the entire database is authorized
   */
  isAllSchema: boolean;
  /**
   * Data source name
   */
  databaseName: string;

  /**
   * schema name
   */
  schemaName: string;

  /**
   *Table name
   */
  tableName: string;

  /**
   * Column filter name
   */
  columnName: string[];

  /**
   * Row filter conditions
   */
  rowFilter: string;

  /**
   * Number of rows that can be viewed at a time
   */
  rowCount: number;

  /**
   * Authorization policy range encoding: DML/DCL_DDL/OTHER
   */
  accessControlPolicyScopeCode: accessControlPolicyScopeCodeType;

  /**
   * Authorization policy encoding: SELECT/UPDATE/INSERT/DELETE/GRANT/ALTER/DROP/CREATE/TRUNCATE/CALL/VIEW
   */
  accessControlPolicyCode: accessControlPolicyCodeType;

  /**
   * Role code: OPERATOR
   */
  roleCode: string;

  /**
   * Permission validity period
   */
  validUntil?: number;
}

export interface IApplyRunScript {
  id?: number;
  name?: string;
  /**
   *Organization ID
   */
  organizationId: number;
  /**
   *Data source ID
   */
  dataSourceId: number;
  /**
   * Database name
   */
  databaseName: string;
  /**
   * schema name
   */
  schemaName: string;
  /**
   * Script content
   */
  scriptContent: string;
  /**
   * Description
   */
  description: string;
  /**
   * Permission validity period
   */
  validUntil?: number;
  /**
   * Is it permanently valid?
   */
  noExpire: BooleanType;
}

/**
 *Application record form
 */
export interface IAccessControlApplyRecordVO {
  /**
   * Primary key
   */
  id: number;

  /**
   *Organization ID
   */
  organizationId: number;

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
  applyUserId: number;

  /**
   * Approval flow ID
   */
  approvalId: number;
  /**
   * Approval user ID
   */
  approveUserId: number;

  /**
   * Application type: ADMIN/OWNER/OPERATOR/SCRIPT
   */
  applyType: ApplyType;

  /**
   * Status APPROVED/REJECTED/PENDING
   */
  status: ApprovalStatusType;

  /**
   * Validity start time
   */
  validFrom: number;

  /**
   * End of validity period
   */
  validUntil: number;

  /**
   * Is it permanently valid: Y/N
   */
  noExpire: BooleanType;

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
   * Approval end time
   */
  finishTime: number;

  /**
   * Update time
   */
  modifyTime: number;

  /**
   *Admin of the application
   */
  dataSourceAdminList: IDataSourceAdminVO[];

  /**
   *Applied data permissions
   */
  dataAccessControlList: IDataAccessControlVO[];

  /**
   * Script permission requested
   */
  scriptAccessControlList: IScriptAccessControlVO[];

  /**
   * Column filter name, multiple separated by commas
   */
  columnNames: string[];

  /**
   * Second level authorization code
   */
  policyVOList: string[];
  /**
   * row filter
   */
  rowFilter: string;

  /**
   * number of rows
   */
  rowCount: number;
}

export interface IAccessControlAuthRecordVO {
  /**
   * Primary key
   */
  id: number;

  /**
   *Organization ID
   */
  organizationId: number;

  /**
   * Application name
   */
  name: string;

  /**
   *Application description
   */
  description: string;
  /**
   * Application type: ADMIN/OWNER/OPERATOR/SCRIPT
   */
  applyType: 'ADMIN' | 'OWNER' | 'OPERATOR' | 'SCRIPT';
  /**
   * Application type
   */
  authType: AuthType;

  authUser: IUserVO;
  /**
   * Authorization subject ID
   */
  authSubjectId: number;

  /**
   * Authorization subject type: USER/ROLE/GROUP
   */
  authSubjectType: 'USER' | 'ROLE' | 'GROUP';

  /**
   *Authorization time
   */
  authTime: number;

  /**
   * Validity start time
   */
  validFrom: number;

  /**
   * End of validity period
   */
  validUntil: number;

  /**
   * Is it permanently valid: Y/N
   */
  noExpire: BooleanType;
  /**
   *
   */
  columnNames: string[];
  /**
   * Authorization policy secondary encoding
   */
  policyVOList: string[];
  /**
   * row filter
   */
  rowFilter: string;

  /**
   * number of rows
   */
  rowCount: number;

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
   *Admin of the application
   */
  dataSourceAdminList: IDataSourceAdminVO[];

  // /**
  //  *Applied data permissions
  //  */
  dataAccessControlList: IDataAccessControlVO[];

  // /**
  //  * Script permission requested
  //  */
  // scriptAccessControlList: IScriptAccessControlVO;
}

/**
 * Data access control list VO
 */
export interface IDataAccessControlVO {
  /**
   * Primary key
   */
  id: number;

  /**
   *Organization ID
   */
  organizationId: number;

  /**
   * Authorization subject ID
   */
  subjectId: number;

  /**
   * Authorization subject type: USER/ROLE/GROUP
   */
  subjectType: string;

  /**
   *Data source ID
   */
  dataSourceId: number;
  dataSourceName: string;
  dataSourceType: string;

  /**
   * Database name
   */
  databaseName: string;

  /**
   * Database schema name
   */
  schemaName: string;

  /**
   * Data range code: TABLE/VIEW
   */
  dataScopeCode: 'TABLE' | 'VIEW';

  /**
   * Access control code: SELECT/UPDATE
   */
  accessControlPolicyCode: accessControlPolicyCodeType;

  /**
   * Application or authorization ID
   */
  grantId: number;

  /**
   * Source of permission: APPLY/AUTHORIZE
   */
  grantReason: 'APPLY' | 'AUTHORIZE';

  /**
   * User ID granting permissions
   */
  grantUserId: string;

  /**
   * Grant time
   */
  grantTime: number;

  /**
   * Status VALID/INVALID
   */
  status: 'VALID' | 'INVALID';

  /**
   * End of validity period
   */
  validUntil: number;

  /**
   * Is it permanently valid: Y/N
   */
  noExpire: 'Y' | 'N';

  /**
   * Listed
   */
  tableName: string;

  /**
   * Listed
   */
  columnName: string;

  /**
   * row filter
   */
  rowFilter: string;

  /**
   * number of rows
   */
  rowCount: number;

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
}

/**
 * Role access control list VO
 */
export interface IDataSourceAdminVO {
  /**
   * Primary key
   */
  id: number;

  /**
   *Organization ID
   */
  organizationId: number;

  /**
   *Administrator ID
   */
  userId: number;

  /**
   * Application or authorization ID
   */
  grantId: number;

  /**
   * Reason for obtaining administrator: OWNER/AUTHORIZE/APPLY
   */
  grantReason: 'OWNER' | 'AUTHORIZE' | 'APPLY';

  /**
   * User ID granting permissions
   */
  grantUserId: string;

  /**
   * Grant time
   */
  grantTime: number;

  /**
   *Data source ID
   */
  dataSourceId: number;
  /**
   * Data source name
   */
  dataSourceName: string;

  /**
   * Delete ID
   */
  deletedId: number;

  /**
   *Deletion time
   */
  deleteTime: number;

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
}

/**
 * Script access control list VO
 */
export interface IScriptAccessControlVO {
  /**
   * Primary key
   */
  id: number;
  /**
   *Organization ID
   */
  organizationId: number;
  /**
   * Script execution submitter
   */
  userId: number;
  /**
   *Data source ID
   */
  dataSourceId: number;
  /**
   * Database name
   */
  databaseName: string;

  /**
   * Database schema name
   */
  schemaName: string;

  /**
   * Script content
   */
  scriptContent: string;

  /**
   * Application or authorization ID
   */
  grantId: number;

  /**
   * User ID granting permissions
   */
  grantUserId: string;

  /**
   * Grant time
   */
  grantTime: number;

  /**
   * Status VALID/INVALID
   */
  status: string;

  /**
   * End of validity period
   */
  validUntil: number;

  /**
   * Script execution time
   */
  executeTime: number;

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
}

export interface IAccessControlPolicyVO {
  /**
   * Strategy coding
   */
  code?: string;
  /**
   * Creation time
   */
  createTime?: number;
  /**
   * Create user ID
   */
  createUserId?: number;
  /**
   * Primary key
   */
  id?: number;
  /**
   * Update time
   */
  modifyTime?: number;
  /**
   * Modify user ID
   */
  modifyUserId?: number;
  /**
   * Strategy name
   */
  name?: string;
  /**
   * Scope encoding: DML, DDL, DCL, VIEW
   */
  scopeCode?: string;
  /**
   * Scope description
   */
  scopeDescription?: string;
}

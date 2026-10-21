// ===================== Common ==================
export enum StatusType {
  INVALID = 'INVALID',
  VALID = 'VALID',
}

export enum RoleType {
  ADMIN = 'ADMIN',
  USER = 'USER',
}

export enum MemberType {
  TEAM = 'TEAM',
  USER = 'USER',
}

// ===================== DataSource ==================

export interface IDataSourceVO {
  /**
   * Connect alias
   */
  alias?: string;
  /**
   * Environment
   */
  environment?: IEnvironmentVO;
  /**
   * environment id
   */
  environmentId?: number;
  /**
   * Primary key id
   */
  id?: number;
  /**
   *Connection address
   */
  url?: string;
}

export interface IEnvironmentVO {
  /**
   * Primary key
   */
  id?: number;
  /**
   * Environment name
   */
  name?: string;
  /**
   * Environment abbreviation
   */
  shortName?: string;
  /**
   * style type
   */
  style?: string;
}

export interface IDataSourceAccessVO {
  /**
   * Authorization object
   */
  accessObject: IDataSourceAccessObjectVO;
  /**
   * Authorization id, distinguish whether it is a user or a team according to the type
   */
  accessObjectId: number;
  /**
   * Authorization type
   */
  accessObjectType: RoleType;
  /**
   * Primary key
   */
  id: number;
}

export interface IDataSourceAccessObjectVO {
  /**
   * The name of the code that belongs to the authorization type, such as user account, team
   * code
   */
  code?: string;
  /**
   * Authorization id, distinguish whether it is a user or a team according to the type
   */
  id?: number;
  /**
   * Code that belongs to the authorization type, such as user name, team name
   */
  name?: string;
  /**
   * Authorization type
   */
  type?: RoleType;
}

// ===================== User ======================

export interface IUserVO {
  /**
   * Primary key
   */
  id: number;

  /**
   * Email
   */
  email: string;
  /**
   * Nickname
   */
  nickName: string;
  /**
   * Password
   */
  password: string;
  /**
   * Role coding
   */
  roleCode: RoleType;
  /**
   * User status
   */
  status: StatusType;
  /**
   * Username
   */
  userName: string;
}

export interface IUserWithTeamVO {
  /**
   * Primary key
   */
  id?: number;
  /**
   *Team
   */
  team?: ITeamVO;
  /**
   * user id
   */
  userId?: number;
}

export interface IUserWithDataSourceVO {
  id?: number;
  /**
   * Data Source
   */
  dataSource?: IDataSourceVO;
  /**
   * user id
   */
  userId?: number;
}

// ===================== Team =====================

export interface ITeamVO {
  id?: number;
  /**
   *Team coding
   */
  code: string;
  /**
   *Team description
   */
  description?: string;
  /**
   *Team name
   */
  name: string;
  /**
   *Team status
   */
  status: StatusType;
}

export interface ITeamWithUserVO {
  id: number;
  teamId: number;
  user: IUserVO;
}

export interface ITeamWithDataSourceVO {
  /**
   * Data Source
   */
  dataSource?: IDataSourceVO;
  /**
   * Primary key
   */
  id: number;
  /**
   * team id
   */
  teamId?: number;
}

// ===================== USER/TEAM =====================
export interface ITeamAndUserVO {
  code?: string;
  id?: number;
  name?: string;
  type?: MemberType;
}

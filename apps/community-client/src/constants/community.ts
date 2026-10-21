import {
  OrgUserRoleCode,
  OrganizationStatusType,
  OrganizationType,
  type IOrganizationVO,
} from '@/typings/enterprise/organization';
import { RoleCodeType } from '@/typings/enterprise/role';
import { type IUserVO } from '@/typings/enterprise/user';

export const COMMUNITY_USER_ID = -1;
export const COMMUNITY_ORGANIZATION_ID = -1;
export const COMMUNITY_DISPLAY_NAME = 'Community Local User';

export const COMMUNITY_ORG: IOrganizationVO = {
  id: COMMUNITY_ORGANIZATION_ID,
  name: 'Community Local Workspace',
  type: OrganizationType.PERSONAL,
  status: OrganizationStatusType.VALID,
  ownerId: COMMUNITY_USER_ID,
  createUserId: COMMUNITY_USER_ID,
  modifyUserId: COMMUNITY_USER_ID,
  createTime: 0,
  modifyTime: 0,
  roleCodes: [OrgUserRoleCode.SUPER_ADMIN, OrgUserRoleCode.ADMIN],
  vip: true,
  area: '',
  industry: '',
  role: '',
  organizationCode: 'community-local',
  organizationAvatar: '',
  seats: 1,
  currentMemberCount: 1,
};

export const COMMUNITY_USER: IUserVO = {
  id: COMMUNITY_USER_ID,
  displayName: COMMUNITY_DISPLAY_NAME,
  roleCode: RoleCodeType.ADMIN,
  avatar: '',
  currentOrganization: COMMUNITY_ORG,
  email: 'community-local@chat2db.local',
  vip: true,
  country: '',
  language: '',
  createTime: 0,
  activated: true,
  trialStartTime: 0,
  networkAbandoned: false,
  subscriptionType: 'Community',
};

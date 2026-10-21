import createRequest from '../base';
import { IPageParams, IPageResponse } from '@/typings';
import {
  IOrganizationUserVO,
  IOrganizationVO,
  IUpdateOrganizationVORequest,
  OrgUserRoleCode,
} from '@/typings/enterprise/organization';
import { Subscription } from '@/typings/enterprise/user';

const prefix = '/api/organization';

/** Query organization details */
const getOrganization = createRequest<{ id: number }, IOrganizationVO>(`${prefix}`, {
  errorLevel: 'toast',
});

const createOrganization = createRequest<Partial<IOrganizationVO>, IOrganizationVO>(`${prefix}/create`, {
  method: 'post',
  errorLevel: 'toast',
});
/** Get the list of organizations */
export const getOrganizationList = createRequest<Partial<IPageParams> | { needCreateOrg: boolean }, IOrganizationVO[]>(
  `${prefix}/list`,
  {
    errorLevel: 'toast',
  },
);

/** Get member list */
export const getOrganizationUserList = createRequest<
  IPageParams | { organizationId: number },
  IPageResponse<IOrganizationUserVO>
>(`${prefix}/list_user`, {
  errorLevel: 'toast',
});

/** Update organization information */
export const updateOrganization = createRequest<IUpdateOrganizationVORequest, IOrganizationVO>(`${prefix}/update`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Get organization token */
// export const getToken = createRequest<{ id: number }, { token: string; id: number }>(`${prefix}/token`);

// /** Update/reset token */
// export const updateToken = createRequest<Partial<{ id: number; resetToken: boolean }>, string>(
//   `${prefix}/update_token`,
//   {
//     method: 'post',
//   },
// );

// Switch organization
export const switchOrg = createRequest<{ id: number }, IOrganizationVO>(`${prefix}/switch_org`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Transfer organization */
export const transferOwner = createRequest<{ id: number; newOwnerId: string }, IOrganizationVO>(
  `${prefix}/transfer_owner`,
  {
    method: 'post',
    errorLevel: 'toast',
  },
);

// /api/organization/add
/** Add administrator members */
export const addAdminUser = createRequest<{ organizationId: number; userId: number }, { url: string }>(
  `${prefix}/add`,
  {
    method: 'post',
    errorLevel: 'toast',
  },
);

// /api/organization/update_user_role
/** Update the user's role in the organization */
export const updateUserRole = createRequest<
  { organizationId: number; userId: number; originalRoleCode: OrgUserRoleCode; targetRoleCode: OrgUserRoleCode },
  boolean
>(`${prefix}/update_user_role`, {
  method: 'post',
  errorLevel: 'toast',
});

// /api/organization/remove_user
/** Remove all roles from the user in the organization */
export const removeUser = createRequest<
  { organizationId: number; userId: number; roleCode: OrgUserRoleCode },
  { url: string }
>(`${prefix}/remove_user`, {
  method: 'post',
  errorLevel: 'toast',
});

const getSubscriptionList =
  __RUNTIME_ENV__ === 'community'
    ? async () => [] as Subscription[]
    : createRequest<void, Subscription[]>(`/api/subscription/list`, {
        method: 'get',
        errorLevel: 'toast',
      });

const cancelSubscription =
  __RUNTIME_ENV__ === 'community'
    ? async () => undefined
    : createRequest<void, void>(`/api/subscription/cancel`, {
        method: 'post',
        errorLevel: 'toast',
      });

const querySubscriptionEndTime =
  __RUNTIME_ENV__ === 'community'
    ? async () => 0
    : createRequest<{ organizationId: number }, number>(`/api/subscription/end_time`);

/**
 * Apply to join an existing organization
 */
const joinOrganization = createRequest<{ organizationCode: string; name: string; reason?: string }, IOrganizationVO>(
  `${prefix}/join`,
  {
    method: 'post',
    errorLevel: 'toast',
  },
);

/**
 * Query team information by team number
 */
const queryOrgByTeamCode = createRequest<{ teamCode: string; inviterId?: number }, IOrganizationVO>(
  `${prefix}/queryByTeamCode`,
  {
    errorLevel: 'toast',
  },
);

const queryOrgDetail = createRequest<
  void,
  {
    currentMemberCount: number;
    seats: number;
  }
>(`${prefix}/detail`, {
  errorLevel: 'toast',
});

export default {
  createOrganization,
  getOrganizationList,
  getOrganizationUserList,
  updateOrganization,
  switchOrg,
  transferOwner,
  getOrganization,
  addAdminUser,
  updateUserRole,
  removeUser,
  getSubscriptionList,
  cancelSubscription,
  joinOrganization,
  queryOrgByTeamCode,
  querySubscriptionEndTime,
  queryOrgDetail,
};

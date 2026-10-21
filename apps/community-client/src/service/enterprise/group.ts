import createRequest from '../base';
import { IPageParams, IPageResponse } from '@/typings';
import { IGroupVO } from '@/typings/enterprise/group';
import { RoleCodeType } from '@/typings/enterprise/role';

const prefix = '/api/group';

/** Create a new group */
const createGroup = createRequest<Partial<IGroupVO>, IGroupVO>(`${prefix}/create`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Add users to groups */
const addUserToGroup = createRequest<{ groupId: number; organizationId: number; userId: number }, IGroupVO>(
  `${prefix}/add_user`,
  {
    method: 'post',
    errorLevel: 'toast',
  },
);

/** Invite users to groups */
const inviteUserToGroup = createRequest<{ groupId: number; organizationId: number; roleCode: RoleCodeType }, IGroupVO>(
  `${prefix}/invite`,
  {
    errorLevel: 'toast',
  },
);

/** User removes group */ //TODO: There should be something wrong with this input parameter
const removeUserFromGroup = createRequest<{ groupId: number; organizationId: number; userId: number }, IGroupVO>(
  `${prefix}/remove_user`,
  {
    errorLevel: 'toast',
  },
);

/** Group details */
const queryGroupDetail = createRequest<{ id: string }, Partial<IGroupVO>>(`${prefix}`, { errorLevel: 'toast' });

/** Get group list */
const queryGroupList = createRequest<IPageParams | { organizationId: number }, IPageResponse<IGroupVO>>(
  `${prefix}/list`,
  {
    errorLevel: 'toast',
  },
);

/** Update group information */
const updateGroup = createRequest<Partial<IGroupVO>, Partial<IGroupVO>>(`${prefix}/update`, {
  method: 'post',
  errorLevel: 'toast',
});

export default {
  createGroup,
  addUserToGroup,
  inviteUserToGroup,
  removeUserFromGroup,
  queryGroupDetail,
  queryGroupList,
  updateGroup,
};

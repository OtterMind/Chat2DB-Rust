import createRequest from '../base';
import { IPageParams, IPageResponse } from '@/typings';
import { IRoleVO } from '@/typings/enterprise/role';

const prefix = '/api/role';

/** Create new role */
const createRole = createRequest<Partial<IRoleVO>, Partial<IRoleVO>>(`${prefix}/create`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Role query */
const queryRole = createRequest<{ id: number }, Partial<IRoleVO>>(`${prefix}`, {
  errorLevel: 'toast',
});

/** Get the role list. */
const queryRoleList = createRequest<IPageParams & { organizationId: number }, IPageResponse<IRoleVO>>(
  `${prefix}/list`,
  {
    errorLevel: 'toast',
  },
);

/** Update role information. */
const updateRole = createRequest<Partial<IRoleVO>, Partial<IRoleVO>>(`${prefix}/update`, {
  method: 'post',
  errorLevel: 'toast',
});

export default { createRole, queryRole, queryRoleList, updateRole };

import {
  IApplyRolePermission,
  IApplyRunScript,
  IAccessControlApplyRecordVO,
  IAccessControlAuthRecordVO,
  IAccessControlPolicyVO,
  IAuthDataAccessRequest,
  IAuthAdminRequest,
  ApplyType,
  IApplyDataAccessRequest,
} from '@/typings/enterprise/permission';
import createRequest from '../base';
import { IPageParams, IPageResponse } from '@/typings';

const prefix = '/api/permission';

// /api/permission/check_data_source_access
/** Query whether there is data source link permission */
const checkDataSourceAccess = createRequest<{ dataSourceId: string; userId: string; organizationId: string }, boolean>(
  `${prefix}/check_data_source_access`,
);

/** Check whether the user has data operation permissions */
const checkDataAccess = createRequest<
  { dataSourceId: string; userId: string; organizationId: number },
  { dataAccess: boolean; noPermissionDetail: string; scriptAccess: boolean }
>(`${prefix}/check_data_access`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Authorized administrator */
const authAdmin = createRequest<IAuthAdminRequest, null>(`${prefix}/auth_admin`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Apply for administrator */
const applyAdmin = createRequest<IApplyRolePermission, null>(`${prefix}/apply_admin`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Authorized data permissions */
const authDataPermission = createRequest<IAuthDataAccessRequest, null>(`${prefix}/auth_data_access`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Apply for data permission */
const applyDataPermission = createRequest<IApplyDataAccessRequest, null>(`${prefix}/apply_data_access`, {
  method: 'post',
  errorLevel: 'toast',
});

/** Apply for script execution permission */
const applyScriptPermission = createRequest<Partial<IApplyRunScript>, null>(`${prefix}/apply_run_script`, {
  method: 'post',
  errorLevel: 'toast',
});

/** View application details */
const queryApplyDetail = createRequest<{ id: number }, IAccessControlApplyRecordVO>(`${prefix}/apply`, {
  errorLevel: 'toast',
});

/** Check my application list */
const queryApplyList = createRequest<
  IPageParams & {
    organizationId: number;
    applyType: ApplyType;
  },
  IPageResponse<IAccessControlApplyRecordVO>
>(`${prefix}/apply_list`, {
  errorLevel: 'toast',
});

/** View authorization details */
const queryAuthDetail = createRequest<{ id: number }, IAccessControlAuthRecordVO>(`${prefix}/auth`, {
  errorLevel: 'toast',
});

/** Query my authorization list */
const queryAuthList = createRequest<
  IPageParams & { organizationId: number },
  IPageResponse<IAccessControlAuthRecordVO>
>(`${prefix}/auth_list`, {
  errorLevel: 'toast',
});

/** View the authorization policy list */
const queryPolicyList = createRequest<{ code?: string; scopeCode?: string }, IAccessControlPolicyVO[]>(
  `${prefix}/policy_list`,
  {
    errorLevel: 'toast',
  },
);

export default {
  checkDataSourceAccess,
  checkDataAccess,
  applyAdmin,
  authAdmin,
  applyDataPermission,
  authDataPermission,
  applyScriptPermission,
  queryApplyDetail,
  queryApplyList,
  queryAuthDetail,
  queryAuthList,
  queryPolicyList,
};

import createRequest from '../base';
import { IPageParams, IPageResponse } from '@/typings';
import { IApprovalProcessVO, IApprovalProcesCanModifyVO, ApprovalStatusType } from '@/typings/enterprise/approval';

const prefix = '/api/approval';

/** Query approval list */
const queryApprovalList = createRequest<
  IPageParams & { organizationId: number; approvalStatus: ApprovalStatusType },
  IPageResponse<IApprovalProcessVO>
>(`${prefix}/list`, {});

/** View approval details */
const queryApprovalDetail = createRequest<{ id: number }, IApprovalProcessVO>(`${prefix}`, { errorLevel: 'toast' });

/** Update approval information */
const updateApproval = createRequest<IApprovalProcesCanModifyVO, Partial<IApprovalProcessVO>>(`${prefix}/update`, {
  method: 'post',
  errorLevel: 'toast',
});

export default { queryApprovalList, queryApprovalDetail, updateApproval };

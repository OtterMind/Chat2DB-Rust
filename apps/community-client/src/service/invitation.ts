import { InvitationOrderVO, WithdrawIncomeParam } from '@/typings/invitation';
import createRequest from './base';

const invitationService =
  __RUNTIME_ENV__ === 'community'
    ? {
        createInvitationCode: async () => false,
        getMyInvitationCode: async () => '',
        getInvitationCodeExit: async () => false,
        getInvitationOrderItem: async () => null as InvitationOrderVO | null,
        withdrawInvitationIncome: async () => false,
      }
    : (() => {
        const createInvitationCode = createRequest<{ code: string }, boolean>(
          '/api/invitation/create_invitation_code',
          {
            method: 'post',
          },
        );

        const getMyInvitationCode = createRequest<void, string>('/api/invitation/get_my_invitation_code');

        const getInvitationCodeExit = createRequest<{ code: string }, boolean>(
          '/api/invitation/get_invitation_code_exit',
        );

        const getInvitationOrderItem = createRequest<void, InvitationOrderVO>(
          '/api/invitation/get_invitation_order_item',
        );

        const withdrawInvitationIncome = createRequest<WithdrawIncomeParam[], boolean>(
          '/api/invitation/withdraw_invitation_income',
          {
            method: 'post',
          },
        );

        return {
          createInvitationCode,
          getMyInvitationCode,
          getInvitationCodeExit,
          getInvitationOrderItem,
          withdrawInvitationIncome,
        };
      })();

export default invitationService;

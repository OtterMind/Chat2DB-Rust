import createRequest from './base';
import { ProductDetailVO, OrderDetail, CreateOrderResponse } from '@/typings/pricing';
import { PayType } from '@/constants/pricing';
import { OrganizationType } from '@/typings/enterprise/organization';

const pricingServices =
  __RUNTIME_ENV__ === 'community'
    ? {
        getProductList: async () => [] as ProductDetailVO[],
        createOrder: async () => {
          throw new Error('Pricing is disabled in community mode');
        },
        getOrder: async () => {
          throw new Error('Pricing is disabled in community mode');
        },
        getOrderList: async () => [],
        cancelSubscription: async () => undefined,
      }
    : (() => {
        const getProductList = createRequest<
          { version?: 2; subscriptionTypes?: any; orgType?: OrganizationType; language?: string },
          ProductDetailVO[]
        >('/api/product/list', { errorLevel: false });

        const createOrder = createRequest<
          {
            id: number;
            paymentMethod?: PayType;
            invitationCode?: string;
            seats?: number;
            organizationId?: number;
            subStartTime?: number;
            freeTrial?: boolean;
            language?: string;
          },
          CreateOrderResponse
        >('/api/pay/create_order', {
          method: 'get',
          errorLevel: 'toast',
        });
        const getOrder = createRequest<{ orderId: string }, OrderDetail>('/api/pay/get_order_status_a', {
          method: 'get',
          errorLevel: 'toast',
        });

        const getOrderList = createRequest<void, any>('/api/order/list', { method: 'get', errorLevel: 'toast' });

        const cancelSubscription = createRequest<void, void>('/api/subscription/cancel', {
          method: 'post',
          errorLevel: 'toast',
        });

        return {
          getProductList,
          createOrder,
          getOrder,
          getOrderList,
          cancelSubscription,
        };
      })();

export default pricingServices;

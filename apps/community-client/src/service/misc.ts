import { BucketTypeEnum, UploadTypeEnum } from '@/typings/upload';
import createRequest from './base';
import { OrderDetail, CreateOrderResponse } from '@/typings/pricing';

const testService = createRequest<null, boolean>('/api/system', { errorLevel: false });
const systemStop = createRequest<void, void>('/api/system/stop', { errorLevel: false, method: 'post' });
const testApiSmooth = createRequest<void, void>('/api/system/get-version-a', { errorLevel: false, method: 'get' });
const uploadFile = createRequest<any, string>('/api/file/upload', { method: 'post' });
const createOrder =
  __RUNTIME_ENV__ === 'community'
    ? async () => {
        throw new Error('Pricing is disabled in community mode');
      }
    : createRequest<{ id: number; seats?: number; invitationCode?: string }, CreateOrderResponse>(
        '/api/pay/create_order',
        { method: 'get' },
      );
const getOrder =
  __RUNTIME_ENV__ === 'community'
    ? async () => {
        throw new Error('Pricing is disabled in community mode');
      }
    : createRequest<{ orderId: string }, OrderDetail>('/api/pay/get_order_status_a', { method: 'get' });

/** Upload CSS */
const getOSSCertificate = createRequest<
  {
    bucketType: BucketTypeEnum;
    uploadType: UploadTypeEnum;
  },
  {
    securityToken: string;
    accessKeySecret: string;
    accessKeyId: string;
    endpoint: string;
    expiration: string;
    requestId: string;
    bucket: string;
    cdn: string;
    fileFolder: string;
  }
>('/api/file/sts');

/**
 * Feedback
 */

const createFeedback = createRequest<
  {
    email?: string;
    phone?: string;
    wechatId?: string;
    review?: string;
    feedbackType?: string;
    description?: string;
    imageUrls?: string;
    uploadLog?: string;
  },
  void
>('/api/feedback/create', { method: 'post', errorLevel: 'toast' });

/**
 * RBI
 */
const fetchSpm =
  __RUNTIME_ENV__ === 'community'
    ? async () => undefined
    : createRequest<
        {
          deviceUuid: string;
          clientVersion: string;
          userAgent: string;
        },
        void
      >('/api/spm/client/create', { method: 'post', errorLevel: false });

/**
 * Filter sql
 */
const characterHandler = createRequest<
  {
    text: string;
  },
  string
>('/api/character/handler', { method: 'post', errorLevel: false });

const activationCode =
  __RUNTIME_ENV__ === 'community'
    ? async () => undefined
    : createRequest<
        {
          activationCode: string;
        },
        void
      >('/api/active/active', { method: 'post', errorLevel: false });

export default {
  testService,
  systemStop,
  testApiSmooth,
  uploadFile,
  createOrder,
  getOrder,
  getOSSCertificate,
  createFeedback,
  fetchSpm,
  characterHandler,
  activationCode,
};

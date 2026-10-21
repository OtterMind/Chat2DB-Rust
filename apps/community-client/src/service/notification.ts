import { IPageParams, IPageResponse } from '@/typings';
import createRequest from './base';
import { ActionType, NotificationVO } from '@/typings/notification';

/**
 * Query the number of unread notifications
 */
const queryUnreadCount = createRequest<void, number>('/api/notification/unread/count', {
  errorLevel: false,
});

/**
 * Query notification list
 */
const queryNotificationList = createRequest<IPageParams, IPageResponse<NotificationVO>>('/api/notification/list', {
  errorLevel: false,
});

/**
 * Handle notifications
 * /api/notification/take/action
 */
const takeNotificationAction = createRequest<
  {
    action: ActionType;
    notificationId: number;
  },
  void
>('/api/notification/take/action', {
  method: 'post',
  errorLevel: false,
});

const queryPopNotification = createRequest<void, NotificationVO>('/api/notification/pop', {
  errorLevel: false,
});

const getNotification = createRequest<{ type: string }, NotificationVO>('/api/notification/get', {
  errorLevel: false,
});

export default {
  queryUnreadCount,
  queryNotificationList,
  takeNotificationAction,
  queryPopNotification,
  getNotification,
};

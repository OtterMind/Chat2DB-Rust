export enum NotificationType {
  CAMPAIGN = 'CAMPAIGN',
  S_CAMPAIGN = 'S_CAMPAIGN',
  NOT_S_CAMPAIGN = 'NOT_S_CAMPAIGN',
  BLOG = 'BLOG',
  DOC = 'DOC',
}

export enum NotificationStatusType {
  Valid = 'VALID',
  Invalid = 'INVALID',
}

export enum ActionType {
  FIRST_QUERY = 'FIRST_QUERY',
  READ = 'READ',
  SHARE = 'SHARE',
  LIKE = 'LIKE',
  DISLIKE = 'DISLIKE',
}

export interface NotificationVO {
  id: number;
  startTime: number;
  endTime: number;
  title: string;
  description: string;
  content: string;
  type: NotificationType;
  status: NotificationStatusType;
  unread: boolean;
  shareUrl: string;
}

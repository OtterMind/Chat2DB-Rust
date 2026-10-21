export enum BucketTypeEnum {
  PUBLIC = 'PUBLIC',
  PRIVATE = 'PRIVATE',
}

export enum UploadTypeEnum {
  /** Upload avatar */
  AVATOR = 'AVATOR',
  /** Feedback pictures */
  FEEDBACK_IMG = 'FEEDBACK_IMG',
  /** Feedback log */
  FEEDBACK_LOG = 'FEEDBACK_LOG',
  /** SQL files or text, csv, etc. files that need to be imported and exported */
  SQLFile = 'SQLFile',
}

export const uploadTypeObject = {
  [UploadTypeEnum.FEEDBACK_IMG]: {
    bucketType: BucketTypeEnum.PRIVATE,
    uploadType: UploadTypeEnum.FEEDBACK_IMG,
  },
  [UploadTypeEnum.AVATOR]: {
    bucketType: BucketTypeEnum.PUBLIC,
    uploadType: UploadTypeEnum.AVATOR,
  },
};

export interface IAliYunOSSCertificate {
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

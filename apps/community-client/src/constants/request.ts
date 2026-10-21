export enum ErrorCode {
  /** Requires login */
  NeedLoggedIn = 'common.needLoggedIn',
  /** No database permissions */
  NoDataAccessPermission = 'NO_DATA_ACCESS_PERMISSION',
  /** No script permissions */
  NoScriptPermission = 'NO_SCRIPT_PERMISSION',
  /** The number of free trials has been exhausted */
  FreeTrialUSageLimit = 'FREE_TRIAL_USAGE_LIMIT',

  /** Offline version - trial expired */
  OfflineTrialExpired = 'api.trialExpired',
  /** Offline version - trial expired */
  OfflineInvalidTrial = 'api.invalidTrial',
  /** Offline version-Invalid device */
  OfflineInvalidDevice = 'api.invalidDevice',
  /** Offline version-license has expired */
  OfflineLicenseExpired = 'api.licenseExpired',

  /** Network error */
  NetworkError = 'api.networkError',

  /** The number of devices bound to the license has reached the upper limit */
  LicenseBindCountExceeds = 'api.licenseBindCountExceeds',

  /** Offline activation does not support AI */
  LicenseNotSupported = 'ai.licenseNotSupported',
}

// Some error codes do not require pop-up prompts
export const ErrorCodesWithoutToast = [
  ErrorCode.NeedLoggedIn,
  ErrorCode.NoDataAccessPermission,
  ErrorCode.NoScriptPermission,
  ErrorCode.FreeTrialUSageLimit,
  ErrorCode.OfflineInvalidDevice,
  ErrorCode.OfflineInvalidTrial,
  ErrorCode.OfflineTrialExpired,
  ErrorCode.OfflineLicenseExpired,
  ErrorCode.NetworkError,
  ErrorCode.LicenseBindCountExceeds,
  ErrorCode.LicenseNotSupported,
];

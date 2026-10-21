export enum GuideDialogStatus {
  FirstLogin, // First login.
  Expired, // Paid plan expiration.
  Subscribed, // Subscribed.
  // TeamSubscribed, // Team subscribed.
  // TeamAddSeat, // Team seat added.
  OfflineTrial, // Offline trial.
  OfflineTrialExpired, // Offline trial expiration.
  OfflineLicenseExpired, // Offline license expiration.
}

export enum LicenseDialogType {
  Activation,
  Unbind,
  DoubleCheck,
}

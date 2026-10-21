export interface ILicenseVO {
  /**
   * Primary key
   */
  id: number;

  /**
   * Encrypted license information
   */
  license: string;

  /**
   * Creation time
   */
  createTime: Date;

  /**
   * Certificate validity type (MONTH/YEAR/PERPETUAL)
   */
  licenseType: string;

  /**
   * Certificate activation time
   */
  licenseActivateTime: Date;

  /**
   * Certificate renewal time
   */
  licenseRenewalTime: Date;

  /**
   * ai expiration time
   */
  aiEndTime: Date;

  /**
   * Number of ai trials per month
   */
  aiMonthCount: number;

  /**
   *The version to which the license applies
   */
  clientVersion: string;

  /**
   *The number of devices that the license can be bound to
   */
  licenseAvailableCount: number;

  /**
   *The number of devices that the license has been bound to
   */
  licenseBindCount: number;

  /**
   * Whether it is possible to generate a certificate
   */
  canGenerateCer: boolean;
}

export interface ILicenseDeviceCerVO {
  /**
   * Primary key
   */
  id: number;

  /**
   *Device name
   */
  deviceName: string;

  /**
   *Device type
   */
  deviceType: string;

  /**
   *Device ID
   */
  deviceId: string;

  /**
   * Encrypted certificate information
   */
  cer: string;

  /**
   *Activation type
   */
  activateType: string;
}

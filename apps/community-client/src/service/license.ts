import { IUserVO } from '@/typings/enterprise/user';
import createRequest from './base';
import { ILicenseDeviceCerVO, ILicenseVO } from '@/typings/license';

const licenseService =
  __RUNTIME_ENV__ === 'community'
    ? {
        startTrial: async () => undefined,
        validLicense: async () => {
          throw new Error('License flow is disabled in community mode');
        },
        removeLicense: async () => undefined,
        sendEmailLicense: async () => undefined,
        checkPasscode: async () => undefined,
        getDeviceId: async () => '',
        activateCer: async () => undefined,
        getLicenseList: async () => [] as ILicenseVO[],
        generateCertificate: async () => '',
        listCertificate: async () => [] as ILicenseDeviceCerVO[],
        deactivateOnline: async () => undefined,
      }
    : (() => {
        const prefix = '/api/license';

        const startTrial = createRequest<void, IUserVO>(`${prefix}/start_trial_a`, {
          method: 'post',
        });

        const validLicense = createRequest<
          {
            license: string;
            deviceId?: string;
          },
          IUserVO & { needDoubleCheck: boolean; codeFromWechat: boolean; doubleCheckDisplayEmail: string }
        >(`${prefix}/activate_license_a`, {
          method: 'post',
        });

        const removeLicense = createRequest<
          {
            license: string;
            deviceId?: string;
          },
          IUserVO
        >(`${prefix}/remove_license`, {
          method: 'post',
        });

        const sendEmailLicense = createRequest<
          {
            licenseKey: string;
          },
          void
        >(`${prefix}/send_sms_license_a`);

        const checkPasscode = createRequest<
          {
            license: string;
            passcode: string;
          },
          IUserVO
        >(`${prefix}/license_check_passcode_a`, {
          method: 'post',
        });

        const getDeviceId = createRequest<void, string>(`${prefix}/get_device_a`);

        const activateCer = createRequest<{ license: string }, IUserVO>(`${prefix}/activate_cer_a`, {
          method: 'post',
        });

        const getLicenseList = createRequest<void, ILicenseVO[]>(`${prefix}/list`);

        const generateCertificate = createRequest<
          { license: string; deviceName: string; deviceType: string; deviceId: string },
          string
        >(`${prefix}/generate_certificate`, {
          method: 'post',
        });

        const listCertificate = createRequest<{ licenseId: string }, ILicenseDeviceCerVO[]>(
          `${prefix}/list_certificate`,
        );

        const deactivateOnline = createRequest<{ deviceId: string }, void>(`${prefix}/deactivate_online`, {
          method: 'post',
        });

        return {
          startTrial,
          validLicense,
          removeLicense,
          sendEmailLicense,
          checkPasscode,
          getDeviceId,
          activateCer,
          getLicenseList,
          generateCertificate,
          listCertificate,
          deactivateOnline,
        };
      })();

export default licenseService;

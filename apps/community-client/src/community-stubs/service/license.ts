const licenseService = {
  startTrial: async () => undefined,
  validLicense: async () => {
    throw new Error('License flow is disabled in community mode');
  },
  removeLicense: async () => undefined,
  sendEmailLicense: async () => undefined,
  checkPasscode: async () => undefined,
  getDeviceId: async () => '',
  activateCer: async () => undefined,
  getLicenseList: async () => [],
  generateCertificate: async () => '',
  listCertificate: async () => [],
  deactivateOnline: async () => undefined,
};

export default licenseService;

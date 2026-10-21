import { IAIConfig } from '@/typings';
import createRequest from './base';

export interface ILatestVersion {
  /**
   * Desktop
   */
  desktop: boolean;
  /**
   * new version
   */
  version: string;
  /**
   * Hot update package address, which can be used to determine whether it is hot update
   */
  hotUpgradeUrl: null | string;
  /**
   *Whether the user chooses manual update or automatic update
   */
  type: 'manual' | 'auto';
  /**
   *Does it need to be updated?
   */
  needUpdate?: boolean;
  /**
   *Download address
   */
  downloadLink?: null | string;
  /**
   * Update log
   */
  updateLog?: null | string;
  /**
   * Whitelist, for testing
   */
  whiteList?: null | string;
}

const getSystemConfig = createRequest<{ code: string }, { code: string; content: string }>(
  '/api/config/system_config/:code',
  { errorLevel: false },
);
const setSystemConfig = createRequest<{ code: string; content: string }, void>('/api/config/system_config', {
  method: 'post',
});

const getAISystemConfig = createRequest<{ aiSqlSource?: string }, IAIConfig>('/api/config/system_config/ai', {
  errorLevel: false,
});

const setAISystemConfig = createRequest<IAIConfig, void>('/api/config/system_config/ai', {
  method: 'post',
});

const getAIWhiteAccess = createRequest<{ apiKey: string }, boolean>('/api/ai/embedding/white/check', {
  method: 'get',
});

// Return the latest version information, or null when no update is available.
const getLatestVersion = createRequest<{ currentVersion: string }, ILatestVersion>('/api/system/get_latest_version', {
  method: 'get',
  errorLevel: false,
});

// Check whether the latest package backend is successfully downloaded
const isUpdateSuccess = createRequest<{ version: string }, boolean>('/api/system/is_update_success', {
  method: 'get',
});

// Tell the backend to download the latest package
const updateDesktopVersion = createRequest<ILatestVersion, boolean>('/api/system/update_desktop_version', {
  method: 'post',
});

// Tell the backend to download the latest package
const setAppUpdateType = createRequest<ILatestVersion['type'], boolean>('/api/system/set_update_type', {
  method: 'post',
});

export default {
  getSystemConfig,
  setSystemConfig,
  getAISystemConfig,
  setAISystemConfig,
  getAIWhiteAccess,
  getLatestVersion,
  isUpdateSuccess,
  updateDesktopVersion,
  setAppUpdateType,
};

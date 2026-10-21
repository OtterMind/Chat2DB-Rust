import { IUserLoginVO, IUserVO } from '@/typings/enterprise/user';
import createRequest from '../base';
import { LoginType } from '@/typings/enterprise/oauth';
import { ServiceAppConfig } from '@/typings/settings';

export interface LoginAddress {
  googleLoginUrl: string;
  githubLoginUrl: string;
  /** Watcha (watcha.cn) passwordless login URL, returned only by the domestic edition. */
  watchaLoginUrl?: string;
}

// Get the Google, GitHub, and Watcha third-party login URLs.
const getLoginUrl = createRequest<void, LoginAddress>('/api/oauth/get_login_url', {
  method: 'get',
  errorLevel: 'toast',
});

/** User email/mobile phone number registration */
const userRegister = createRequest<Partial<IUserVO> | { passcode: string; registerType: LoginType }, IUserVO>(
  `/api/oauth/register_a`,
  {
    method: 'post',
    errorLevel: 'toast',
  },
);

/** User login */
const userLogin = createRequest<IUserLoginVO, any>(`/api/oauth/login_a`, {
  method: 'post',
  errorLevel: false,
});

/** Get current user login information */
const getUserInfo = createRequest<void, IUserVO>('/api/oauth/user_a', { method: 'get', errorLevel: false });

/** User logout */
const userLogout = createRequest<void, void>('/api/oauth/logout_a', {
  method: 'post',
  errorLevel: 'toast',
});

/** Reset password */
const resetPassword = createRequest<
  {
    email: string;
    passcode: string;
    newPassword: string;
  },
  void
>(`/api/oauth/reset_password`, {
  method: 'post',
  errorLevel: 'toast',
});

// APP configuration
const getAppConfig = createRequest<void, ServiceAppConfig>('/api/oauth/get_app_config', {
  method: 'get',
  errorLevel: false,
});

// Get WeChat QR code
const getWechatQrCode = createRequest<void, { wechatQrCodeUrl: string; token: string; tip: string }>(
  '/api/oauth/wechat_qr_a',
  {
    method: 'get',
    errorLevel: false,
  },
);

// Get WeChat login status
const getWechatLoginStatus = createRequest<{ token: string }, { status: string }>('/api/oauth/wechat_login_a', {
  method: 'get',
  errorLevel: false,
});

export default {
  getLoginUrl,
  userRegister,
  userLogin,
  resetPassword,
  userLogout,
  getUserInfo,
  getAppConfig,
  getWechatQrCode,
  getWechatLoginStatus,
};

import { AppConfig } from '@/typings/appConfig';

export const APP_URL_CONFIG_OVERSEAS = {
  WEBSITE_URL: `https://chat2db.ai`,
  DOWNLOAD_URL: `https://chat2db.ai/download`,
  WEBSITE_PRICING_URL: `https://chat2db.ai/pricing`,
  CHAT2DB_APP_URL: `https://app.chat2db.ai`,
  CHAT2DB_PRICING_URL: `https://app.chat2db.ai/price`,
  DOCS_URL: `https://chat2db.ai/resources/docs/start-guide/getting-started`,
  CHANGE_LOG_URL: `https://chat2db.ai/resources/changelog`,
  SERVICE_AGREEMENT: `https://chat2db.ai/resources/docs/service/service`,
  PRIVACY_POLICY: `https://chat2db.ai/resources/docs/service/privacy`,
  MEMBER_AGREEMENT: `https://chat2db.ai/resources/docs/service/member`,
  CURRENCY_SYMBOL: '$',
};

export const APP_URL_CONFIG_CHINA = {
  WEBSITE_URL: `https://chat2db-ai.com`,
  DOWNLOAD_URL: `https://chat2db-ai.com/download`,
  WEBSITE_PRICING_URL: `https://chat2db-ai.com/pricing`,
  CHAT2DB_APP_URL: `https://app.chat2db-ai.com`,
  CHAT2DB_PRICING_URL: `https://app.chat2db-ai.com/price`,
  DOCS_URL: `https://chat2db-ai.com/resources/docs/start-guide/getting-started`,
  CHANGE_LOG_URL: `https://chat2db-ai.com/resources/changelog`,
  SERVICE_AGREEMENT: `https://chat2db-ai.com/resources/docs/service/service`,
  PRIVACY_POLICY: `https://chat2db-ai.com/resources/docs/service/privacy`,
  MEMBER_AGREEMENT: `https://chat2db-ai.com/resources/docs/service/member`,
  CURRENCY_SYMBOL: '¥',
};

export const APP_URL_CONFIG_COMMUNITY = {
  WEBSITE_URL: `https://chat2db.ai`,
  DOWNLOAD_URL: `https://chat2db.ai/download`,
  WEBSITE_PRICING_URL: '',
  CHAT2DB_APP_URL: '',
  CHAT2DB_PRICING_URL: '',
  DOCS_URL: `https://chat2db.ai/resources/docs/start-guide/getting-started`,
  CHANGE_LOG_URL: `https://chat2db.ai/resources/changelog`,
  SERVICE_AGREEMENT: '',
  PRIVACY_POLICY: `https://chat2db.ai/resources/docs/service/privacy`,
  MEMBER_AGREEMENT: '',
  CURRENCY_SYMBOL: '',
};

export const getAppConfig = (isCN: boolean) => {
  return isCN ? APP_URL_CONFIG_CHINA : APP_URL_CONFIG_OVERSEAS;
};

export const appConfigMap: {
  [key: string]: AppConfig;
} = {
  'chat2db-pro': {
    name: 'chat2db-pro',
    capitalName: 'Chat2DB-Pro',
    displayName: 'Chat2DB Pro',
  },
  'chat2db-pro-test': {
    name: 'chat2db-pro-test',
    capitalName: 'Chat2DB-Pro-Test',
    displayName: 'Chat2DB Pro Test',
  },
  'chat2db-local': {
    name: 'chat2db-local',
    capitalName: 'Chat2DB-Local',
    displayName: 'Chat2DB Local',
  },
  'chat2db-local-test': {
    name: 'chat2db-local-test',
    capitalName: 'Chat2DB-Local-Test',
    displayName: 'Chat2DB Local Test',
  },
  'chat2db-community': {
    name: 'chat2db-community',
    capitalName: 'Chat2DB-Community',
    displayName: 'Chat2DB Community',
  },
  'chat2db-community-test': {
    name: 'chat2db-community-test',
    capitalName: 'Chat2DB-Community-Test',
    displayName: 'Chat2DB Community Test',
  },
};

export const APP_CONFIG: AppConfig = appConfigMap[__APP_NAME__] || appConfigMap['chat2db-pro'];

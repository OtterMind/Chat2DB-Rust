import createRequest from './base';

export enum NetworkProxyMode {
  NO_PROXY = 'NO_PROXY',
  SYSTEM = 'SYSTEM',
  MANUAL = 'MANUAL',
}

export enum NetworkProxyType {
  HTTP = 'HTTP',
  SOCKS = 'SOCKS',
}

export interface INetworkProxySettings {
  mode: NetworkProxyMode;
  proxyType: NetworkProxyType;
  host?: string;
  port?: number;
  noProxyHosts?: string;
  restartRequired?: boolean;
}

export interface INetworkProxyTestRequest {
  settings: INetworkProxySettings;
  testUrl?: string;
}

const prefix = '/api/network/proxy';

const get = createRequest<void, INetworkProxySettings>(prefix, {
  errorLevel: false,
});

const save = createRequest<INetworkProxySettings, INetworkProxySettings>(prefix, {
  method: 'post',
});

const test = createRequest<INetworkProxyTestRequest, boolean>(`${prefix}/test`, {
  method: 'post',
});

export default {
  get,
  save,
  test,
};

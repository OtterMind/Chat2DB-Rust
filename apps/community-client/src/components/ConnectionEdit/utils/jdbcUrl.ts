function isBracketedHost(host: string) {
  return host.startsWith('[') && host.endsWith(']');
}

export function stripJdbcHostBrackets(host: string) {
  if (isBracketedHost(host)) {
    return host.slice(1, -1);
  }
  return host;
}

function isIpv6LiteralHost(host: string) {
  const hostValue = stripJdbcHostBrackets(host.trim());
  if (!hostValue.includes(':')) {
    return false;
  }
  if (hostValue.includes('://') || hostValue.includes('/') || hostValue.includes('?') || hostValue.includes('#')) {
    return false;
  }

  const colonCount = (hostValue.match(/:/g) || []).length;
  if (colonCount < 2) {
    return false;
  }

  return /^[0-9a-fA-F:.]+(%[\w.-]+)?$/.test(hostValue);
}

export function formatJdbcHostForUrl(host: any) {
  const hostValue = host == null ? '' : String(host);
  const bareHost = stripJdbcHostBrackets(hostValue);
  if (isIpv6LiteralHost(bareHost)) {
    return `[${bareHost}]`;
  }
  return bareHost;
}

export function normalizeJdbcHostFromUrl(host: any) {
  const hostValue = host == null ? '' : String(host);
  return stripJdbcHostBrackets(hostValue);
}

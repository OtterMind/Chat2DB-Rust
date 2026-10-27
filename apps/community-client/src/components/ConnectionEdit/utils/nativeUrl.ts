/**
 * Community renders JDBC URLs, while the Rust native drivers take a plain
 * scheme (mysql://host:3306/db). Native drivers accept both forms, so the field
 * and the stored value use the native form; only legacy rows stay JDBC and are
 * converted for display.
 */
const NATIVE_PREFIXES: ReadonlyArray<readonly [string, string]> = [
  ['jdbc:mysql://', 'mysql://'],
  ['jdbc:postgresql://', 'postgresql://'],
  ['jdbc:sqlserver://', 'sqlserver://'],
];

export interface IDriverConfigLike {
  jdbcDriver?: string;
  jdbcDriverClass?: string;
}

export function isNativeDriverConfig(config?: IDriverConfigLike | null): boolean {
  const driverClass = config?.jdbcDriverClass;
  return typeof driverClass === 'string' && driverClass.startsWith('rust:');
}

/** JDBC URL to the native driver's shorthand for the URL field. */

/** Oracle keeps distinct SID and service forms, so it needs its own mapping. */
const ORACLE_JDBC_PREFIX = 'jdbc:oracle:thin:@';
const ORACLE_NATIVE_PREFIX = 'oracle://';

function oracleToDisplay(url: string): string {
  const lower = url.toLowerCase();
  if (lower.startsWith(ORACLE_NATIVE_PREFIX)) {
    return url;
  }
  if (lower.startsWith(ORACLE_JDBC_PREFIX)) {
    const rest = url.slice(ORACLE_JDBC_PREFIX.length);
    // Service form uses a double slash: jdbc:oracle:thin:@//host:port/service
    return ORACLE_NATIVE_PREFIX + (rest.startsWith('//') ? rest.slice(2) : rest);
  }
  return url;
}

function oracleToStored(url: string): string {
  const lower = url.toLowerCase();
  if (lower.startsWith(ORACLE_JDBC_PREFIX)) {
    return url;
  }
  if (lower.startsWith(ORACLE_NATIVE_PREFIX)) {
    const rest = url.slice(ORACLE_NATIVE_PREFIX.length);
    // A slash after host:port means the service form; otherwise it is a SID.
    const isService = rest.includes('/');
    return ORACLE_JDBC_PREFIX + (isService ? '//' : '') + rest;
  }
  return url;
}

export function toDisplayUrl(url: string): string {
  if (!url) {
    return url;
  }
  const lower = url.toLowerCase();
  const oracle = oracleToDisplay(url);
  if (oracle !== url) {
    return oracle;
  }
  for (const [jdbcPrefix, nativePrefix] of NATIVE_PREFIXES) {
    if (lower.startsWith(jdbcPrefix)) {
      return nativePrefix + url.slice(jdbcPrefix.length);
    }
    if (lower.startsWith(nativePrefix)) {
      return url;
    }
  }
  return url;
}

/** Normalises whatever the user typed to the native form the drivers store. */
export function toStoredUrl(url: string): string {
  return toDisplayUrl(url);
}

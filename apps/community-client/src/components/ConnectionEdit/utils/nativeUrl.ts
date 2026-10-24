/**
 * Community renders and submits JDBC URLs, while the Rust native drivers take a
 * plain scheme (mysql://host:3306/db). The stored value stays JDBC so the
 * compatibility path keeps working; only the field content is swapped.
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
export function toDisplayUrl(url: string): string {
  if (!url) {
    return url;
  }
  const lower = url.toLowerCase();
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

/** Native shorthand back to the JDBC form the backend stores. */
export function toStoredUrl(url: string): string {
  if (!url) {
    return url;
  }
  const lower = url.toLowerCase();
  for (const [jdbcPrefix, nativePrefix] of NATIVE_PREFIXES) {
    if (lower.startsWith(nativePrefix)) {
      return jdbcPrefix + url.slice(nativePrefix.length);
    }
    if (lower.startsWith(jdbcPrefix)) {
      return url;
    }
  }
  return url;
}

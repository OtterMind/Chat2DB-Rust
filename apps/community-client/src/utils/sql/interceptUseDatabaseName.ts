/**
 * Intercept use databaseName
 * Only intercept SQL in the `use xxx` or `use xxx;` format.
 * If intercepted, return databaseName, otherwise return false
 */
export function interceptUseDatabaseName(sql: string) {
  const reg = /^use\s+[\w-]+;?$/i;
  return reg.test(sql) ? sql.replace(/^use\s+|;$/gi, '') : false;
}

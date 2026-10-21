import { DatabaseTypeCode } from '@/constants';
import sqlServer from '@/service/sql';

export function formatSql(sql: string, dbType?: DatabaseTypeCode) {
  return new Promise((r: (sql: string) => void) => {
    sqlServer
      .sqlFormat({
        sql,
        dbType,
      })
      .then((res) => {
        r(res);
      });
  });
}

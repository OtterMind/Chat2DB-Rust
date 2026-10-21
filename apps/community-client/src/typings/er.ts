export interface IERTableColumn {
  name: string;
  columnType: string;
  primaryKey?: boolean;
  comment: string;
}

export interface IERTableForeignKey {
  pkTableName: string;
  pkColumnName: string;
  fkTableName: string;
  fkColumnName: string;
}

export interface IERTableDetail {
  name: string;
  comment: string;
  columnList: IERTableColumn[];
  foreignKeyList: IERTableForeignKey[];
}

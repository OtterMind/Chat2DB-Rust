export interface IColumn {
  name: string;
  dataType: string;
  columnType: string; // Column type, such as varchar(100), double(10,6)
  nullable: boolean;
  primaryKey: boolean;
  defaultValue: string;
  autoIncrement: boolean;
  numericPrecision: number;
  numericScale: number;
  characterMaximumLength: number;
  comment: string;
}

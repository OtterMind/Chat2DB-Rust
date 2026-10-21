import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import intelliSense from '@/constants/IntelliSense';
import { DatabaseTypeCode } from '@/constants/common';
import i18n from '@/i18n';
import { isNumber } from 'lodash';
import sqlService from '@/service/sql';
// import { compatibleDataBaseName } from '@/utils/database';

interface IFieldInfo {
  filedName: string;
  tableName: string;
}

interface ITableInfo {
  dataSourceId: number;
// With supportSchema enabled, schemaName is set and databaseName is empty.
  schemaName?: string;
// Without supportSchema, databaseName is set and schemaName is empty.
  databaseName?: string;
  tableName: string;
  filedInfo?: IFieldInfo[];
}
interface ISchemaInfo {
  schemaName: string;
// With supportDatabase enabled, databaseName is set.
  databaseName?: string;
// With supportTable enabled, dataSource is set.
  dataSourceName?: string;
  dataSourceId?: number;
  tableInfo?: ITableInfo[];
}

interface IDatabaseInfo {
  dataSourceId: number;
  dataSourceName: string;
  databaseName: string;
  schemaInfo?: ISchemaInfo[];
  tableInfo?: ITableInfo[];
}
interface IDataSourceInfo {
  dataSourceId: number;
  dataSourceName: string;
  supportDatabase: boolean;
  supportSchema: boolean;
  databaseInfo?: IDatabaseInfo[];
  schemaInfo?: ISchemaInfo[];
}

enum SORT_TEXT {
  Database = '0',
  Schema = '1',
  Table = '2',
  Column = '3',
  Keyword = '4',
}

export class CompletionProvider implements monaco.languages.CompletionItemProvider {
  /** Whether the current window is active. */
  isActive: boolean;
  /** All information for the current database. */
  dbInfo: IDataSourceInfo;
  tempDBInfo: {
    databaseNameList: Omit<IDatabaseInfo, 'tableInfo' | 'schemaInfo'>[];
    schemaList: Omit<ISchemaInfo, 'tableInfo'>[];
    tableList: Omit<ITableInfo, 'filedInfo'>[];
    fieldList: IFieldInfo[];
  };
  /** Trigger key. */
  triggerCharacters = ['.'];
  constructor({ boundInfo, isActive }) {
    this.isActive = isActive;
    this.dbInfo = {
      dataSourceId: boundInfo.dataSourceId,
      dataSourceName: boundInfo.dataSourceName,
      supportDatabase: boundInfo.supportDatabase,
      supportSchema: boundInfo.supportSchema,
      databaseInfo: [],
      schemaInfo: [],
    };
    this.tempDBInfo = {
      databaseNameList: [],
      schemaList: [],
      tableList: [],
      fieldList: [],
    };
  }

  setActive(isActive: boolean) {
    this.isActive = isActive;
  }

  setDBInfo(dbInfo: IDataSourceInfo) {
    this.dbInfo = dbInfo;
  }

  setDatabaseNameList(databaseInfo: IDatabaseInfo[]) {
    this.dbInfo?.databaseInfo = databaseInfo;
  }

  /** Get the data source bound to the current window. */
  getCurrentDataSourceInfo(): IDatabaseInfo | null {
    const { dataSourceId } = this.boundInfo;
    if (!isNumber(dataSourceId)) {
      return null;
    }
    return this.dbInfo[dataSourceId as number];
  }

  /** Get database names for the current window's data source. */
  getCurrentDatabaseNameList(): IDatabaseInfo[] {
    const dataSourceInfo = this.getCurrentDataSourceInfo();
    return dataSourceInfo?.databaseInfo ?? [];
  }

  /** Get schemas for the current window's data source. */
  getCurrentSchemaList(): ISchemaInfo[] {
    const dataSourceInfo = this.getCurrentDataSourceInfo();
    if (!dataSourceInfo?.supportSchema) {
      // No schema.
      return [];
    }
    if (dataSourceInfo.supportDatabase) {
      // Both database and schema are present.
      const databaseNameList = this.getCurrentDatabaseNameList();
      return databaseNameList.reduce((acc: ISchemaInfo[], cur) => {
        const { schemaInfo } = cur;
        if (schemaInfo) {
          acc.push(...schemaInfo);
        }
        return acc;
      }, []);
    } else {
    // Only schema is present.
      return dataSourceInfo.schemaInfo || [];
    }
  }

  /** Get tables for the current window's data source. */
  getCurrentTableList(): ITableInfo[] {
    const dataSourceInfo = this.getCurrentDataSourceInfo();
    if (!dataSourceInfo) return [];

      // When only Database exists, tables are direct children of the database.
    if (dataSourceInfo.supportDatabase && !dataSourceInfo.supportSchema) {
      const databaseNameList = this.getCurrentDatabaseNameList();
      return databaseNameList.reduce((acc: ITableInfo[], cur) => {
        const { tableInfo } = cur;
        if (tableInfo) {
          acc.push(...tableInfo);
        }
        return acc;
      }, []);
    }
      // When schema exists, tables are children of the schema.
    const schemaList = this.getCurrentSchemaList();
    return schemaList.reduce((acc: ITableInfo[], cur) => {
      const { tableInfo } = cur;
      if (tableInfo) {
        acc.push(...tableInfo);
      }
      return acc;
    }, []);
  }

  // Get fields for the current window's data source.
  getCurrentFieldList(): IFieldInfo[] {
    const tableList = this.getCurrentTableList();
    return tableList.reduce((acc: IFieldInfo[], cur) => {
      const { filedInfo } = cur;
      if (filedInfo) {
        acc.push(...filedInfo);
      }
      return acc;
    }, []);
  }

  /** Format database completions. */
  formatDatabaseNameSuggestion(databaseNameList?: IDatabaseInfo[]) {
    return (databaseNameList || []).map((item) => ({
      label: {
        label: item.databaseName,
        detail: item.dataSourceName ? `(${item.dataSourceName})` : null,
        description: i18n('sqlEditor.text.databaseName'),
      },
      kind: monaco.languages.CompletionItemKind.Property,
      sortText: SORT_TEXT.Database,
      insertText: item.databaseName,
      range: undefined,
    }));
  }

  /** Format schema-list completions. */
  formatSchemaSuggestion(schemaList: ISchemaInfo[]) {
    return schemaList.map((item) => ({
      label: {
        label: item.schemaName,
        detail: item.databaseName ? `(${item.databaseName})` : `(${item.dataSourceName})`,
        description: i18n('sqlEditor.text.schemaName'),
      },
      kind: monaco.languages.CompletionItemKind.Folder,
      sortText: SORT_TEXT.Schema,
      insertText: item.schemaName,
    }));
  }

  /** Format table-list completions. */
  formatTableSuggestion(tableList: ITableInfo[]) {
    return tableList.map((item) => ({
      label: {
        label: item.tableName,
        detail: item.schemaName ? `(${item.schemaName})` : `(${item.databaseName})`,
        description: i18n('sqlEditor.text.tableName'),
      },
      kind: monaco.languages.CompletionItemKind.Folder,
      sortText: SORT_TEXT.Table,
      insertText: item.tableName,
    }));
  }

  /** Format field-list completions. */
  formatFieldSuggestion(fieldList: IFieldInfo[]) {
    return fieldList.map((item) => ({
      label: {
        label: item.filedName,
        detail: item.tableName ? `(${item.tableName})` : null,
        description: i18n('sqlEditor.text.fieldName'),
      },
      kind: monaco.languages.CompletionItemKind.Field,
      insertText: item.filedName,
      sortText: SORT_TEXT.Column,
    }));
  }

  /** Field completions. */
  async getFieldsSuggestion(_tableName?: string, tableInfo?: ITableInfo) {
    const curDatabaseInfo = this.dbInfo[this.boundInfo.dataSourceId!].databaseInfo.find(
      (db) => db.databaseName === this.boundInfo.databaseName,
    );

    // Read tableName from boundInfo when it is not provided.
    if (!_tableName) {
      const fieldList: IFieldInfo[] = [];
      curDatabaseInfo?.tableInfo?.map((table) => {
        const { filedInfo } = table;
        fieldList.push(...(filedInfo || []));
      });
      return this.formatFieldSuggestion(fieldList);
    }

    const curTableInfo = (curDatabaseInfo?.tableInfo || []).find((table) => table.tableName === tableName);

    const { dataSourceId, databaseName, schemaName, tableName, filedInfo } = tableInfo || curTableInfo || {};
    if (!filedInfo) {
      const data = await sqlService.getAllFieldByTable({
        dataSourceId,
        databaseName,
        schemaName,
        tableName,
      });
      const _filedInfo = (data || []).map((f) => ({ filedName: f.name, tableName: f.tableName }));
      if (tableInfo) {
        tableInfo.filedInfo = _filedInfo;
      } else if (curTableInfo) {
        curTableInfo.filedInfo = _filedInfo;
      }
    }
    return (tableInfo?.filedInfo || []).map((item) => ({
      label: {
        label: item.filedName,
        detail: tableName ? `(${tableName})` : null,
        description: i18n('sqlEditor.text.fieldName'),
      },
      kind: monaco.languages.CompletionItemKind.Field,
      insertText: item.filedName,
      sortText: SORT_TEXT.Column,
    }));
  }

  /** Table completions. */
  async getTableSuggestion(databaseInfo?: IDatabaseInfo): Promise<monaco.languages.CompletionItem[]> {
    return this.formatTableSuggestion(databaseInfo?.tableInfo || this.getCurrentTableList());
  }

  /** Schema completions. */
  async getSchemaSuggection() {}

  /** Database completions. */
  getDatabaseNameSuggestion(): monaco.languages.CompletionItem[] {
    const { dataSourceId } = this.boundInfo;
    if (!isNumber(dataSourceId)) {
      return [];
    }
    const dbInfo = this.dbInfo[dataSourceId as number];
    return this.formatDatabaseNameSuggestion(dbInfo.databaseInfo);
  }

  /** Keyword completions. */
  getKeywordSuggestion(): monaco.languages.CompletionItem[] {
    const commonIntelliSense = Object.values(intelliSense).find((v) => v.type === this.boundInfo.databaseType);
    const intelliSenseMySQL = Object.values(intelliSense).find((v) => v.type === DatabaseTypeCode.MYSQL);
    const { keywords, functions } = commonIntelliSense || intelliSenseMySQL || {};

    const keywordsSuggestions = (keywords || []).map((key: any) => ({
      label: {
        label: key,
        detail: '',
        description: i18n('sqlEditor.text.keyword'),
      },
      kind: monaco.languages.CompletionItemKind.Text,
      sortText: SORT_TEXT.Keyword,
      insertText: key,
    }));

    const functionsSuggestions = (functions || []).map((key: any) => ({
      label: {
        label: key,
        detail: '',
        description: i18n('sqlEditor.text.function'),
      },
      kind: monaco.languages.CompletionItemKind.Function,
      sortText: SORT_TEXT.Keyword,
      insertText: key,
    }));
    return [...keywordsSuggestions, ...functionsSuggestions];
  }

  /** Determine whether completions are needed. */
  checkNeedProvide() {
    // Do not provide completions while inactive.
    if (!this.isActive) {
      return false;
    }

    // Do not provide completions without a data source.
    if (!isNumber(this.boundInfo.dataSourceId)) {
      return false;
    }
    return true;
  }

  async provideCompletionItems(model: monaco.editor.ITextModel, position: monaco.Position) {
    console.log('Begin provideCompletionItems');

    if (!this.checkNeedProvide()) {
      return {
        suggestions: [],
      };
    }

    const lastToken = this.getLastToken(model, position);
    const lastSecondToken = this.getLastSecondToken(model, position);
    console.log('lastToken', lastToken);
    console.log('lastSecondToken', lastSecondToken);

    // ================= Enter Match =================
    if (lastSecondToken === 'database') {
      // Database completions.
      return {
        suggestions: this.getDatabaseNameSuggestion(),
      };
    } else if (lastToken.endsWith('.')) {
      // <database>.<schema>.<table> || <database>.<table> || <schema>.<table> || <table>.<field>
      console.log('Enter Database.Table');
      return {
        suggestions: this.handleEndWithDot(model, position),
      };
    } else if (['from', 'join', 'update'].includes(lastSecondToken)) {
      // Database and table suggestions.
      console.log('Enter from/join/update');
      const tableList = await this.getTableSuggestion();
      return {
        suggestions: [...this.getDatabaseNameSuggestion(), ...tableList],
      };
    } else if (
      ['select', 'where', 'order by', 'group by', 'by', 'and', 'or', 'having', 'distinct', 'on'].includes(
        lastSecondToken,
      )
    ) {
      console.log('Enter select/where/order by/group by/by/and/or/having/distinct/on');
      const fieldList = await this.getFieldsSuggestion();
      return {
        suggestions: [...fieldList],
      };
    } else {
      // Fallback completions.
      // Keyword completions.
      console.log('Enter default');
      return {
        suggestions: [
          ...this.getDatabaseNameSuggestion(),
          // ...this.getTableSuggestion(),
          ...this.getKeywordSuggestion(),
        ],
      };
    }
  }

  handleThisTokenType() {}

  /** Whether the input ends with a dot. */
  async handleEndWithDot(model: monaco.editor.ITextModel, position: monaco.Position) {
    // <database>.<schema>.<table> || <database>.<table> || <schema>.<table> || <table>.<field>
    const lastToken = this.getLastToken(model, position);
    const tokenNoDot = lastToken.slice(0, lastToken.length - 1);

    const dataSourceId = this.boundInfo.dataSourceId as number;
    const dbInfo = this.dbInfo[dataSourceId as number];

    const databaseInfo = dbInfo.databaseInfo.find((db) => db.databaseName === tokenNoDot.replace(/^.*,/g, ''));
    if (!databaseInfo) {
      return;
    }
        // TODO: Handle the possible schema case.
    if (dbInfo.supportSchema) {
          // Schema is supported.
      console.log('Enter supportSchema');
    }
    if (dbInfo.supportDatabase && !dbInfo.supportSchema) {
          // Schema is not supported.
      console.log('Enter not supportSchema');
      const suggestions = await this.getTableSuggestion(databaseInfo);
      console.log('suggest', suggestions);
      return suggestions;
    }
    return [];
  }

  // Get the previous token, which often contains the user's matching character.
  getLastToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return tokens[tokens.length - 1].toLowerCase();
  }

  // Get the penultimate token, which is normally the previous word.
  getLastSecondToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return (tokens[tokens.length - 2] || '').toLowerCase();
  }

  // Text on the current line before the cursor.
  getTextBeforePointer(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  // Text on all lines before the cursor.
  getTextBeforePointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: 1,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  // Text on all lines after the cursor.
  getTextAfterPointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: model.getLineCount(),
      endColumn: model.getLineMaxColumn(model.getLineCount()),
    });
  }

  // Text after the first semicolon before the cursor.
  getTextBeforeSemicolon(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    const text = model.getValueInRange({
      startLineNumber: 1,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
    return text.split(';').pop() || '';
  }

  // Text before the first semicolon after the cursor.
  getTextAfterSemicolon(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    const text = model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: model.getLineCount(),
      endColumn: model.getLineMaxColumn(model.getLineCount()),
    });
    return text.split(';').shift() || '';
  }

  // Get the complete SQL around the cursor.
  getSQLTextByPosition(model: monaco.editor.ITextModel, position: monaco.Position) {
    return this.getTextBeforeSemicolon(model, position) + this.getTextAfterSemicolon(model, position);
  }

  /**
   * Get all table names and aliases from SQL.
   * @param {*} sqlText SQL string.
   */
  getTableNameAndTableAlia(sqlText) {
    const regTableAliaFrom =
      /(^|(\s+))from\s+([^\s]+(\s+||(\s+as\s+))[^\s]+(\s+|,)\s*)+(\s+(where|left|right|full|join|inner|union))?/gi;
    const regTableAliaJoin = /(^|(\s+))join\s+([^\s]+)\s+(as\s+)?([^\s]+)\s+on/gi;
    const regTableAliaFromList = sqlText.match(regTableAliaFrom) ? sqlText.match(regTableAliaFrom) : [];
    const regTableAliaJoinList = sqlText.match(regTableAliaJoin) ? sqlText.match(regTableAliaJoin) : [];
    const strList = [
      ...regTableAliaFromList.map((item) =>
        item
          .replace(/(^|(\s+))from\s+/gi, '')
          .replace(/\s+(where|left|right|full|join|inner|union)((\s+.*?$)|$)/gi, '')
          .replace(/\s+as\s+/gi, ' ')
          .trim(),
      ),
      ...regTableAliaJoinList.map((item) =>
        item
          .replace(/(^|(\s+))join\s+/gi, '')
          .replace(/\s+on((\s+.*?$)|$)/, '')
          .replace(/\s+as\s+/gi, ' ')
          .trim(),
      ),
    ];
    const tableList = [];
    strList.map((tableAndAlia) => {
      tableAndAlia.split(',').forEach((item) => {
        const tableName = item.trim().split(/\s+/)[0];
        const tableAlia = item.trim().split(/\s+/)[1];
        tableList.push({
          tableName,
          tableAlia,
        });
      });
    });
    console.log('sqlText', sqlText);
    console.log('strList', strList);
    console.log('regTableAliaFromList', regTableAliaFromList);
    console.log('regTableAliaJoinList', regTableAliaJoinList);
    console.log('regTableAliaFromList', regTableAliaFromList);
    console.log('regTableAliaJoinList', regTableAliaJoinList);
    console.log('获取sql中所有的表名和别名', tableList);
    return tableList;
  }
}

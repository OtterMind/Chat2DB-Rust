import { IBoundInfo } from '@/typings';
import { isNumber } from 'lodash';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import MagicStickService from '@/service/magicStick';
import { ChatSourceType } from '@/constants/chat';
import i18n from '@/i18n';
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
enum DatabaseFieldType {
  DATASOURCE = 'DATASOURCE',
  DATABASE = 'DATABASE',
  SCHEMA = 'SCHEMA',
  TABLE = 'TABLE',
  COLUMN = 'COLUMN',
}

export enum SORT_TEXT {
  DATABASE = '01',
  SCHEMA = '02',
  TABLE = '03',
  COLUMN = '04',
  KEYWORD = '05',
  FUNCTION = '06',
}
export class CompletionProvider implements monaco.languages.CompletionItemProvider {
  boundInfo: IBoundInfo;
  isActive: boolean;
  triggerCharacters?: string[];
  fetchController: AbortController | null;
  constructor({ boundInfo, isActive }) {
    this.isActive = isActive;
    this.boundInfo = boundInfo;
    this.triggerCharacters = ['.'];
    this.fetchController = null;
  }
  setActive(isActive: boolean) {
    this.isActive = isActive;
  }
  setBoundInfo(boundInfo: IBoundInfo) {
    this.boundInfo = boundInfo;
  }

  /** Determine whether completions are needed. */
  checkNeedProvider() {
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

  async provideCompletionItems(
    model: monaco.editor.ITextModel,
    position: monaco.Position,
  ): monaco.languages.ProviderResult<monaco.languages.CompletionList> {
    if (!this.checkNeedProvider()) {
      return {
        suggestions: [],
      };
    }
    // Trigger completions only on spaces.
    const textBeforePointer = this.getTextBeforePointer(model, position);
    if (!textBeforePointer.endsWith('.')) {
      return;
    }
  /** Replacement range. */
    const range = {
      startLineNumber: position.lineNumber,
      startColumn: position.column,
      endLineNumber: position.lineNumber,
      endColumn: position.column,
    };
    const beforeContext = this.getTextBeforeSemicolon(model, position);
    const afterContext = this.getTextAfterSemicolon(model, position);
    this.fetchController = new AbortController();
    const data = await MagicStickService.queryPrompt(
      {
        dataSourceId: this.boundInfo.dataSourceId,
        databaseName: this.boundInfo.databaseName,
        schemaName: this.boundInfo.schemaName,
        source: ChatSourceType.DATASOURCE_CHAT,
        beforeContext,
        afterContext,
      },
      {
        signal: this.fetchController.signal,
      },
    );

    const { selectPrompt } = data || {};
    const suggestions = (selectPrompt?.items || []).reduce((acc: monaco.languages.CompletionItem[], item) => {
      const { type, value } = item;
      // const lastWord = lastToken.slice(0, lastToken.length - 1);
      const lastWord = selectPrompt.alias || selectPrompt.value;
      const beforeWordIsSchema = data?.selectPrompt?.type === DatabaseFieldType.SCHEMA;
      switch (type) {
        case DatabaseFieldType.DATABASE:
          acc.push({
            ...this.formatDatabaseSuggestion({ databaseName: value, dataSourceName: lastWord }),
            range,
          });
          break;
        case DatabaseFieldType.SCHEMA:
          acc.push({
            ...this.formatSchemaSuggestion({ schemaName: value, databaseName: lastWord }),
            range,
          });
          break;
        case DatabaseFieldType.TABLE:
          acc.push({
            ...this.formatTableSuggestion({
              tableName: value,
              schemaName: beforeWordIsSchema ? lastWord : undefined,
              databaseName: beforeWordIsSchema ? undefined : lastWord,
            }),
            range,
            command: {
              id: 'addFieldList',
              title: 'addFieldList',
              arguments: [
                {
                  ...this.boundInfo,
                },
              ],
            },
          });
          break;
        case DatabaseFieldType.COLUMN:
          acc.push({
            ...this.formatFieldSuggestion({ filedName: value, tableName: lastWord }),
            range,
          });
          break;
        default:
          break;
      }
      return acc;
    }, []);
    return {
      suggestions,
    };
  }

  /** Format database completions. */
  formatDatabaseSuggestion({ databaseName, dataSourceName }: Pick<IDatabaseInfo, 'dataSourceName' | 'databaseName'>) {
    return {
      label: {
        label: databaseName,
        detail: dataSourceName ? `(${dataSourceName})` : '',
        description: i18n('sqlEditor.text.databaseName'),
      },
      kind: monaco.languages.CompletionItemKind.Folder,
      sortText: SORT_TEXT.DATABASE,
      insertText: databaseName,
    };
  }

  /** Format schema completions. */
  formatSchemaSuggestion({ schemaName, databaseName }: Pick<ISchemaInfo, 'schemaName' | 'databaseName'>) {
    return {
      label: {
        label: schemaName,
        detail: databaseName ? `${databaseName}` : '',
        description: i18n('sqlEditor.text.schemaName'),
      },
      kind: monaco.languages.CompletionItemKind.Folder,
      sortText: SORT_TEXT.SCHEMA,
      insertText: schemaName,
    };
  }
  /** Format table completions. */
  formatTableSuggestion({
    tableName,
    schemaName,
    databaseName,
  }: Pick<ITableInfo, 'tableName' | 'schemaName' | 'databaseName'>) {
    console.log('Enter formatTableSuggestion', tableName, schemaName, databaseName);
    return {
      label: {
        label: tableName,
        detail: schemaName ? `(${schemaName})` : databaseName ? `(${databaseName})` : '',
        description: i18n('sqlEditor.text.tableName'),
      },
      kind: monaco.languages.CompletionItemKind.Folder,
      insertText: tableName,
      sortText: SORT_TEXT.TABLE,
    };
  }

  /** Format field completions. */
  formatFieldSuggestion({ filedName, tableName }: IFieldInfo) {
    return {
      label: {
        label: filedName,
        detail: tableName ? `(${tableName})` : '',
        description: i18n('sqlEditor.text.fieldName'),
      },
      kind: monaco.languages.CompletionItemKind.Field,
      insertText: filedName,
      sortText: SORT_TEXT.COLUMN,
    };
  }

  /** Get the previous token, which often contains the user's matching character. */
  getLastToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return tokens[tokens.length - 1].toLowerCase();
  }

  /** Get the penultimate token, which is normally the previous word. */
  getLastSecondToken(model: monaco.editor.ITextModel, position: monaco.Position) {
    const textBeforePointer = this.getTextBeforePointer(model, position);
    const tokens = textBeforePointer.trim().split(/\s+/);
    return (tokens[tokens.length - 2] || '').toLowerCase();
  }

  /** Text on the current line before the cursor. */
  getTextBeforePointer(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  /** Text on all lines before the cursor. */
  getTextBeforePointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: 1,
      startColumn: 0,
      endLineNumber: lineNumber,
      endColumn: column,
    });
  }
  /** Text on all lines after the cursor. */
  getTextAfterPointerMulti(model: monaco.editor.ITextModel, position: monaco.Position) {
    const { lineNumber, column } = position;
    return model.getValueInRange({
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: model.getLineCount(),
      endColumn: model.getLineMaxColumn(model.getLineCount()),
    });
  }

  /** Text after the first semicolon before the cursor. */
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

  /** Text before the first semicolon after the cursor. */
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

  /** Get the complete SQL around the cursor. */
  getSQLTextByPosition(model: monaco.editor.ITextModel, position: monaco.Position) {
    return this.getTextBeforeSemicolon(model, position) + this.getTextAfterSemicolon(model, position);
  }
}

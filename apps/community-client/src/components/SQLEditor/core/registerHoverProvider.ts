import * as monaco from 'monaco-editor';
import React from 'react';
import { IBoundInfo } from '@/typings';
import SQLParserService from '@/service/sqlParser';
import { SqlStatement, StatementValidTypeEnum } from '@/typings/sqlParser';

interface HoverProviderParams {
  dbInfo: IBoundInfo;
  sqlStatementListRef: React.MutableRefObject<SqlStatement[]>;
}

export const onHoverEditor = async (
  editor: monaco.editor.IStandaloneCodeEditor,
  mouse: monaco.editor.IEditorMouseEvent,
  p: HoverProviderParams,
) => {
  const { dbInfo, sqlStatementListRef } = p;

  if (
    !dbInfo.dataSourceId
    // ||
    // (dbInfo.supportDatabase && !dbInfo.databaseName) ||
    // (dbInfo.supportSchema && !dbInfo.schemaName)
  )
    return null;

  const model = editor.getModel();
  if (!model) return null;

  const position = mouse.target.position;
  if (!position) return null;

  const wordObj = model.getWordAtPosition(position);
  if (!wordObj || !wordObj.word) return null;

  const currentStatement = (sqlStatementListRef.current || []).find(
    (stmt) => stmt.sqlStartRowNum <= position.lineNumber && stmt.sqlEndRowNum >= position.lineNumber,
  );

  if (!currentStatement || currentStatement?.statementType === StatementValidTypeEnum.INVALID) return null;

  const identifier = (currentStatement?.identifiers || []).find((idf) => idf.name === wordObj.word);
  if (!identifier) return null;

  return await SQLParserService.queryHover({
    consoleId: dbInfo.consoleId!,
    sql: model.getValue(),
    currentStatement: currentStatement,
    hoverIdentifier: identifier,
    dataSourceId: dbInfo.dataSourceId,
    databaseName: dbInfo.databaseName,
    schemaName: dbInfo.schemaName,
  });
};

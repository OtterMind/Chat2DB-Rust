import { DatabaseTypeCode } from '@/constants/common';
import { IBoundInfo } from '@/typings';
import { SimpleIdentifier, SqlStatement, SqlTypeEnum, StatementValidTypeEnum } from '@/typings/sqlParser';
import type * as monaco from 'monaco-editor';
import { IRange } from '../type';

export interface EditorTableIdentifier {
  dataSourceId: number;
  dataSourceName?: string;
  databaseType?: IBoundInfo['databaseType'];
  databaseName?: string;
  schemaName?: string;
  objectType: EditorDatabaseObjectType;
  tableName: string;
  identifier: SimpleIdentifier;
  identifierQuoted?: boolean;
}

export type EditorDatabaseObjectType = 'TABLE' | 'VIEW' | 'FUNCTION' | 'PROCEDURE';

interface PositionRange {
  startRow: number;
  startCol: number;
  endRow: number;
  endCol: number;
}

export function findTableIdentifierAtPosition(
  position: monaco.IPosition | null | undefined,
  sqlStatementList: SqlStatement[],
  dbInfo: IBoundInfo,
): EditorTableIdentifier | null {
  if (!position || !dbInfo.dataSourceId) {
    return null;
  }

  const statement = (sqlStatementList || []).find(
    (item) => item.statementType === StatementValidTypeEnum.VALID && isPositionInStatement(position, item),
  );
  if (!statement) {
    return null;
  }

  const identifier = (statement.identifiers || []).find((item) => {
    if (!isNavigableDatabaseObjectIdentifier(statement, item)) {
      return false;
    }
    return isPositionInRange(position, getTableIdentifierRange(statement, item));
  });
  if (!identifier?.name) {
    return null;
  }

  const qualifiedParts = getQualifiedIdentifierParts(statement, identifier);
  const normalizedQualifiedPartNames = qualifiedParts.map((part) =>
    normalizeIdentifierNameByDatabaseType(part.name, dbInfo.databaseType, part.quoted),
  );
  const tableIdentifierPart = qualifiedParts[qualifiedParts.length - 1];
  const objectType = getEditorDatabaseObjectType(identifier.type);
  const normalizedTableIdentifierPart =
    tableIdentifierPart &&
    normalizeIdentifierNameByDatabaseType(tableIdentifierPart.name, dbInfo.databaseType, tableIdentifierPart.quoted);
  const tableName =
    normalizedTableIdentifierPart ||
    normalizeIdentifierNameByDatabaseType(identifier.name, dbInfo.databaseType, isQuotedIdentifier(identifier.name));
  const databaseName =
    getDatabaseName(normalizedQualifiedPartNames) ||
    normalizeIdentifierNameByDatabaseType(
      identifier.identifierDatabase,
      dbInfo.databaseType,
      isQuotedIdentifier(identifier.identifierDatabase),
    ) ||
    dbInfo.databaseName;
  const schemaName =
    getSchemaName(normalizedQualifiedPartNames, identifier.identifierDatabase) ||
    normalizeIdentifierNameByDatabaseType(
      identifier.identifierSchema,
      dbInfo.databaseType,
      isQuotedIdentifier(identifier.identifierSchema),
    ) ||
    dbInfo.schemaName;

  return {
    dataSourceId: dbInfo.dataSourceId,
    dataSourceName: dbInfo.dataSourceName,
    databaseType: dbInfo.databaseType,
    databaseName,
    schemaName,
    objectType,
    tableName,
    identifier,
    identifierQuoted: tableIdentifierPart?.quoted || isQuotedIdentifier(identifier.name),
  };
}

export function getTableIdentifierUnderlineRanges(sqlStatementList: SqlStatement[]): IRange[] {
  return (sqlStatementList || [])
    .filter((statement) => statement.statementType === StatementValidTypeEnum.VALID)
    .flatMap((statement) => {
      return (statement.identifiers || [])
        .filter((identifier) => isNavigableDatabaseObjectIdentifier(statement, identifier))
        .map((identifier) => getTableIdentifierRange(statement, identifier))
        .filter((range) => !!range.startRow && !!range.endRow)
        .map((range) => ({
          startLineNumber: range.startRow,
          startColumn: range.startCol,
          endLineNumber: range.endRow,
          endColumn: range.endCol,
        }));
    });
}

function isSupportedDatabaseObjectIdentifier(identifier: SimpleIdentifier) {
  return ['TABLE', 'VIEW', 'FUNCTION', 'UDF_FUNCTION', 'PROCEDURE'].includes(identifier.type?.toUpperCase?.());
}

function isNavigableDatabaseObjectIdentifier(statement: SqlStatement, identifier: SimpleIdentifier) {
  if (!identifier.name || !isSupportedDatabaseObjectIdentifier(identifier)) {
    return false;
  }
  return !isAliasQualifierIdentifier(statement, identifier) && !isStatementDefinitionIdentifier(statement, identifier);
}

function isAliasQualifierIdentifier(statement: SqlStatement, identifier: SimpleIdentifier) {
  if (!isRelationIdentifier(identifier)) {
    return false;
  }

  const identifierName = normalizeIdentifierName(identifier.name);
  const aliasNames = getRelationAliasNames(statement);
  if (!aliasNames.some((aliasName) => isIdentifierNameMatch(aliasName, identifierName))) {
    return false;
  }

  const parserRange = getParserIdentifierRange(identifier);
  if (getRelationAliasRanges(statement).some((aliasRange) => isRangeIntersect(aliasRange, parserRange))) {
    return true;
  }

  return isIdentifierFollowedByDot(statement, parserRange);
}

function isRelationIdentifier(identifier: SimpleIdentifier) {
  return ['TABLE', 'VIEW'].includes(identifier.type?.toUpperCase?.());
}

function getRelationAliasNames(statement: SqlStatement) {
  return (statement.identifiers || [])
    .filter(isRelationIdentifier)
    .map((identifier) => normalizeIdentifierName(identifier.alias))
    .filter(Boolean);
}

function getRelationAliasRanges(statement: SqlStatement) {
  return (statement.identifiers || [])
    .filter((identifier) => isRelationIdentifier(identifier) && !!identifier.alias)
    .map(getAliasIdentifierRange)
    .filter((range) => !!range.startRow && !!range.endRow);
}

function getAliasIdentifierRange(identifier: SimpleIdentifier): PositionRange {
  return {
    startRow: identifier.aliasStartRowNum,
    startCol: identifier.aliasStartColNum,
    endRow: identifier.aliasEndRowNum,
    endCol: identifier.aliasEndColNum,
  };
}

function isIdentifierFollowedByDot(statement: SqlStatement, range: PositionRange) {
  if (!range.endRow || range.startRow !== range.endRow) {
    return false;
  }

  const line = getStatementLine(statement, range.endRow);
  let index = range.endCol - 1;
  while (index < line.length && /\s/.test(line[index])) {
    index += 1;
  }
  return line[index] === '.';
}

function isStatementDefinitionIdentifier(statement: SqlStatement, identifier: SimpleIdentifier) {
  const targetObjectType = getCreateStatementTargetObjectType(statement.type);
  if (!targetObjectType) {
    return false;
  }

  const targetIdentifier = (statement.identifiers || []).find(
    (item) => getEditorDatabaseObjectType(item.type) === targetObjectType,
  );
  if (!targetIdentifier) {
    return false;
  }

  return getParserIdentifierRange(targetIdentifier).startRow === getParserIdentifierRange(identifier).startRow &&
    getParserIdentifierRange(targetIdentifier).startCol === getParserIdentifierRange(identifier).startCol &&
    getParserIdentifierRange(targetIdentifier).endRow === getParserIdentifierRange(identifier).endRow &&
    getParserIdentifierRange(targetIdentifier).endCol === getParserIdentifierRange(identifier).endCol;
}

function getCreateStatementTargetObjectType(statementType: SqlStatement['type']): EditorDatabaseObjectType | null {
  if (statementType === SqlTypeEnum.CREATE_TABLE) {
    return 'TABLE';
  }
  if (statementType === SqlTypeEnum.CREATE_VIEW) {
    return 'VIEW';
  }
  if (statementType === SqlTypeEnum.CREATE_FUNCTION) {
    return 'FUNCTION';
  }
  if (statementType === SqlTypeEnum.CREATE_PROCEDURE) {
    return 'PROCEDURE';
  }
  return null;
}

function getEditorDatabaseObjectType(type: string): EditorDatabaseObjectType {
  const normalizedType = type?.toUpperCase?.();
  if (normalizedType === 'VIEW') {
    return 'VIEW';
  }
  if (normalizedType === 'FUNCTION' || normalizedType === 'UDF_FUNCTION') {
    return 'FUNCTION';
  }
  if (normalizedType === 'PROCEDURE') {
    return 'PROCEDURE';
  }
  return 'TABLE';
}

function isPositionInStatement(position: monaco.IPosition, statement: SqlStatement) {
  const afterStart =
    position.lineNumber > statement.sqlStartRowNum ||
    (position.lineNumber === statement.sqlStartRowNum && position.column >= statement.sqlStartColNum);
  const beforeEnd =
    position.lineNumber < statement.sqlEndRowNum ||
    (position.lineNumber === statement.sqlEndRowNum && position.column <= statement.sqlEndColNum);

  return afterStart && beforeEnd;
}

function getTableIdentifierRange(statement: SqlStatement, identifier: SimpleIdentifier): PositionRange {
  const parserRange = getParserIdentifierRange(identifier);
  const sqlRange = getSqlQualifiedIdentifierRange(statement, identifier, parserRange);
  return sqlRange || parserRange;
}

function getQualifiedIdentifierParts(statement: SqlStatement, identifier: SimpleIdentifier) {
  const parserRange = getParserIdentifierRange(identifier);
  const sqlRange = getSqlQualifiedIdentifierRange(statement, identifier, parserRange);
  if (!sqlRange || sqlRange.startRow !== sqlRange.endRow) {
    return [];
  }

  const line = getStatementLine(statement, sqlRange.startRow);
  if (!line) {
    return [];
  }

  return splitQualifiedIdentifier(line.slice(sqlRange.startCol - 1, sqlRange.endCol - 1)).map(
    createQualifiedIdentifierPart,
  );
}

function getParserIdentifierRange(identifier: SimpleIdentifier): PositionRange {
  return {
    startRow: identifier.identifierStartRowNum,
    startCol: identifier.identifierStartColNum,
    endRow: identifier.identifierEndRowNum,
    endCol: identifier.identifierEndColNum,
  };
}

function getSqlQualifiedIdentifierRange(
  statement: SqlStatement,
  identifier: SimpleIdentifier,
  parserRange: PositionRange,
): PositionRange | null {
  if (!statement.sql || !parserRange.startRow || !parserRange.endRow || parserRange.startRow !== parserRange.endRow) {
    return null;
  }

  const line = getStatementLine(statement, parserRange.startRow);
  if (!line) {
    return null;
  }

  const tableName = normalizeIdentifierName(identifier.identifierTable || identifier.name);
  const identifierRegExp =
    /(?:"(?:""|[^"])*"|`(?:``|[^`])*`|\[[^\]]+\]|[A-Za-z_][\w$]*)(?:\s*\.\s*(?:"(?:""|[^"])*"|`(?:``|[^`])*`|\[[^\]]+\]|[A-Za-z_][\w$]*))*/g;
  let match: RegExpExecArray | null;
  while ((match = identifierRegExp.exec(line))) {
    const startCol = match.index + 1;
    const endCol = match.index + match[0].length + 1;
    const parts = splitQualifiedIdentifier(match[0]).map(normalizeIdentifierName);
    const lastPart = parts[parts.length - 1];
    const candidateRange = {
      startRow: parserRange.startRow,
      startCol,
      endRow: parserRange.startRow,
      endCol,
    };

    if (isIdentifierNameMatch(lastPart, tableName) && isRangeIntersect(parserRange, candidateRange)) {
      return candidateRange;
    }
  }

  return null;
}

function getStatementLine(statement: SqlStatement, row: number) {
  const index = row - statement.sqlStartRowNum;
  return statement.sql.split(/\r?\n/)[index] || '';
}

function splitQualifiedIdentifier(value: string) {
  const parts: string[] = [];
  let current = '';
  let quote: string | null = null;
  let bracket = false;

  for (let i = 0; i < value.length; i += 1) {
    const char = value[i];
    if (quote) {
      current += char;
      if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (bracket) {
      current += char;
      if (char === ']') {
        bracket = false;
      }
      continue;
    }
    if (char === '"' || char === '`') {
      quote = char;
      current += char;
      continue;
    }
    if (char === '[') {
      bracket = true;
      current += char;
      continue;
    }
    if (char === '.') {
      parts.push(current.trim());
      current = '';
      continue;
    }
    current += char;
  }
  parts.push(current.trim());
  return parts.filter(Boolean);
}

function createQualifiedIdentifierPart(rawValue: string) {
  return {
    rawValue,
    name: normalizeIdentifierName(rawValue),
    quoted: isQuotedIdentifier(rawValue),
  };
}

function normalizeIdentifierName(value?: string) {
  const name = (value || '').trim();
  if (
    (name.startsWith('`') && name.endsWith('`')) ||
    (name.startsWith('"') && name.endsWith('"')) ||
    (name.startsWith('[') && name.endsWith(']'))
  ) {
    return name.slice(1, -1);
  }
  return name;
}

function normalizeIdentifierNameByDatabaseType(
  value: string | undefined,
  databaseType: IBoundInfo['databaseType'] | undefined,
  quoted: boolean,
) {
  const name = normalizeIdentifierName(value);
  if (name && isOracleLikeDatabase(databaseType) && !quoted) {
    return name.toUpperCase();
  }
  return name;
}

function isOracleLikeDatabase(databaseType: IBoundInfo['databaseType'] | undefined) {
  return databaseType === DatabaseTypeCode.ORACLE || databaseType === DatabaseTypeCode.OCEANBASE_ORACLE;
}

function isQuotedIdentifier(value?: string) {
  const name = (value || '').trim();
  return (
    (name.startsWith('`') && name.endsWith('`')) ||
    (name.startsWith('"') && name.endsWith('"')) ||
    (name.startsWith('[') && name.endsWith(']'))
  );
}

function isIdentifierNameMatch(candidateName: string | undefined, identifierName: string | undefined) {
  if (candidateName === identifierName) {
    return true;
  }
  return !!candidateName && !!identifierName && candidateName.toLowerCase() === identifierName.toLowerCase();
}

function getDatabaseName(qualifiedParts: string[]) {
  if (qualifiedParts.length >= 3) {
    return qualifiedParts[qualifiedParts.length - 3];
  }
  return '';
}

function getSchemaName(qualifiedParts: string[], identifierDatabase?: string) {
  if (qualifiedParts.length >= 3) {
    return qualifiedParts[qualifiedParts.length - 2];
  }
  if (qualifiedParts.length === 2 && !identifierDatabase) {
    return qualifiedParts[0];
  }
  return '';
}

function isRangeIntersect(left: PositionRange, right: PositionRange) {
  return (
    isPositionInRange({ lineNumber: left.startRow, column: left.startCol }, right) ||
    isPositionInRange({ lineNumber: left.endRow, column: left.endCol }, right) ||
    isPositionInRange({ lineNumber: right.startRow, column: right.startCol }, left) ||
    isPositionInRange({ lineNumber: right.endRow, column: right.endCol }, left)
  );
}

function isPositionInRange(position: monaco.IPosition, range: PositionRange) {
  if (!range.startRow || !range.endRow) {
    return false;
  }

  const afterStart =
    position.lineNumber > range.startRow ||
    (position.lineNumber === range.startRow && position.column >= range.startCol);
  const beforeEnd =
    position.lineNumber < range.endRow || (position.lineNumber === range.endRow && position.column <= range.endCol);

  return afterStart && beforeEnd;
}

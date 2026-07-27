import type {
  BuildCommunityNamespaceSqlRequest,
  CommunityDatabase,
} from './backend';

export type CommunityNamespaceOperationKind =
  BuildCommunityNamespaceSqlRequest['operation']['kind'];

export interface CommunityNamespaceDraft {
  kind: CommunityNamespaceOperationKind;
  sourceDatabase: CommunityDatabase;
  databaseName: string;
  oldDatabaseName: string;
  newDatabaseName: string;
  schemaDatabaseName: string;
  schemaName: string;
  oldSchemaName: string;
  newSchemaName: string;
  comment: string;
  charset: string;
  collation: string;
  owner: string;
}

export type CommunityNamespaceBuildResult =
  | { ok: true; request: BuildCommunityNamespaceSqlRequest }
  | { ok: false; error: string };

export interface CommunityNamespaceRequestIdentity {
  sequence: number;
  scope: string;
}

const MAX_IDENTIFIER_BYTES = 512;
const MAX_PROPERTY_BYTES = 4096;
const MAX_COMMENT_BYTES = 65536;

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function databaseMetadata(
  name: string,
  draft: CommunityNamespaceDraft,
): CommunityDatabase {
  return {
    name,
    comment: draft.comment.trim(),
    charset: draft.charset.trim(),
    collation: draft.collation.trim(),
    owner: draft.owner.trim(),
    system: false,
  };
}

function sourceDatabaseMetadata(
  name: string,
  draft: CommunityNamespaceDraft,
): CommunityDatabase {
  if (name === draft.sourceDatabase.name) return { ...draft.sourceDatabase, name };
  return {
    name,
    comment: '',
    charset: '',
    collation: '',
    owner: '',
    system: false,
  };
}

function validateRequiredIdentifier(value: string, label: string): string | null {
  if (!value) return `${label} is required.`;
  if (utf8Length(value) > MAX_IDENTIFIER_BYTES) {
    return `${label} exceeds the Community identifier limit.`;
  }
  if (
    [...value].some((character) => /[\u0000-\u001f\u007f]/.test(character))
    || /[.;'"`\[\]]/.test(value)
    || value.includes('--')
  ) return `${label} contains unsupported characters.`;
  return null;
}

function validateProperty(value: string, label: string): string | null {
  const normalized = value.trim();
  if (utf8Length(normalized) > MAX_PROPERTY_BYTES) {
    return `${label} exceeds the Community property limit.`;
  }
  if (!/^[\p{L}\p{N}_$@-]*$/u.test(normalized) || normalized.includes('--')) {
    return `${label} contains unsupported characters.`;
  }
  return null;
}

function validateComment(value: string): string | null {
  const normalized = value.trim();
  if (utf8Length(normalized) > MAX_COMMENT_BYTES) {
    return 'Comment exceeds the Community comment limit.';
  }
  if (
    [...normalized].some((character) => /[\u0000-\u001f\u007f]/.test(character))
    || /['\\]/.test(normalized)
    || normalized.includes('--')
    || normalized.includes('/*')
    || normalized.includes('*/')
  ) return 'Comment contains unsupported characters.';
  return null;
}

function validateDatabaseProperties(draft: CommunityNamespaceDraft): string | null {
  return validateComment(draft.comment)
    ?? validateProperty(draft.charset, 'Charset')
    ?? validateProperty(draft.collation, 'Collation')
    ?? validateProperty(draft.owner, 'Owner');
}

export function createCommunityNamespaceDraft(
  database: CommunityDatabase | undefined,
  schemaName: string,
  initialKind: CommunityNamespaceOperationKind,
): CommunityNamespaceDraft {
  return {
    kind: initialKind,
    sourceDatabase: database ?? {
      name: '',
      comment: '',
      charset: '',
      collation: '',
      owner: '',
      system: false,
    },
    databaseName: '',
    oldDatabaseName: database?.name ?? '',
    newDatabaseName: '',
    schemaDatabaseName: database?.name ?? '',
    schemaName: '',
    oldSchemaName: schemaName,
    newSchemaName: '',
    comment: '',
    charset: '',
    collation: '',
    owner: '',
  };
}

export function buildCommunityNamespaceRequest(
  databaseType: string,
  draft: CommunityNamespaceDraft,
): CommunityNamespaceBuildResult {
  const databaseName = draft.databaseName.trim();
  const oldDatabaseName = draft.oldDatabaseName.trim();
  const newDatabaseName = draft.newDatabaseName.trim();
  const schemaDatabaseName = draft.schemaDatabaseName.trim();
  const schemaName = draft.schemaName.trim();
  const oldSchemaName = draft.oldSchemaName.trim();
  const newSchemaName = draft.newSchemaName.trim();
  let operation: BuildCommunityNamespaceSqlRequest['operation'];
  switch (draft.kind) {
    case 'createDatabase': {
      const error = validateRequiredIdentifier(databaseName, 'Database name');
      if (error) return { ok: false, error };
      const propertyError = validateDatabaseProperties(draft);
      if (propertyError) return { ok: false, error: propertyError };
      operation = { kind: draft.kind, database: databaseMetadata(databaseName, draft) };
      break;
    }
    case 'alterDatabase': {
      const error = validateRequiredIdentifier(oldDatabaseName, 'Current database name')
        ?? validateRequiredIdentifier(newDatabaseName, 'New database name');
      if (error) return { ok: false, error };
      const propertyError = validateDatabaseProperties(draft);
      if (propertyError) return { ok: false, error: propertyError };
      operation = {
        kind: draft.kind,
        oldDatabase: sourceDatabaseMetadata(oldDatabaseName, draft),
        newDatabase: databaseMetadata(newDatabaseName, draft),
      };
      break;
    }
    case 'dropDatabase':
    case 'useDatabase': {
      const error = validateRequiredIdentifier(oldDatabaseName, 'Database name');
      if (error) return { ok: false, error };
      operation = { kind: draft.kind, databaseName: oldDatabaseName };
      break;
    }
    case 'createSchema': {
      const error = validateRequiredIdentifier(schemaDatabaseName, 'Database name')
        ?? validateRequiredIdentifier(schemaName, 'Schema name');
      if (error) return { ok: false, error };
      const propertyError = validateComment(draft.comment)
        ?? validateProperty(draft.owner, 'Owner');
      if (propertyError) return { ok: false, error: propertyError };
      operation = {
        kind: draft.kind,
        schema: {
          databaseName: schemaDatabaseName,
          name: schemaName,
          comment: draft.comment.trim(),
          owner: draft.owner.trim(),
          system: false,
        },
      };
      break;
    }
    case 'alterSchema': {
      const error = validateRequiredIdentifier(oldSchemaName, 'Current schema name')
        ?? validateRequiredIdentifier(newSchemaName, 'New schema name');
      if (error) return { ok: false, error };
      operation = { kind: draft.kind, oldSchemaName, newSchemaName };
      break;
    }
    case 'dropSchema': {
      const error = validateRequiredIdentifier(oldSchemaName, 'Schema name');
      if (error) return { ok: false, error };
      operation = { kind: draft.kind, schemaName: oldSchemaName };
      break;
    }
  }
  return { ok: true, request: { databaseType, operation } };
}

export function isCurrentCommunityNamespaceRequest(
  request: CommunityNamespaceRequestIdentity,
  latestSequence: number,
  currentScope: string | null,
): boolean {
  return request.sequence === latestSequence && request.scope === currentScope;
}

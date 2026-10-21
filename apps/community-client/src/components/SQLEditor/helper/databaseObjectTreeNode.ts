import { TreeNodeType } from '@/constants/tree';
import { TreeNodeData } from '@/typings/tree';
import { EditorTableIdentifier } from './tableIdentifier';
import { DatabaseTypeCode } from '@/constants/common';
import type { Key } from 'react';

export interface DatabaseObjectTreeNodeMatchParams {
  treeNodeType: TreeNodeType;
  dataSourceId?: number;
  databaseType?: EditorTableIdentifier['databaseType'];
  databaseName?: string;
  schemaName?: string;
  name?: string;
}

export const getCandidateTreeNodeTypes = (treeNodeType: TreeNodeType) => {
  if (treeNodeType === TreeNodeType.TABLE) {
    return [TreeNodeType.TABLE, TreeNodeType.VIEW];
  }
  return [treeNodeType];
};

export const getTreeNodeTypeByObjectType = (objectType: EditorTableIdentifier['objectType']): TreeNodeType => {
  if (objectType === 'VIEW') {
    return TreeNodeType.VIEW;
  }
  if (objectType === 'FUNCTION') {
    return TreeNodeType.FUNCTION;
  }
  if (objectType === 'PROCEDURE') {
    return TreeNodeType.PROCEDURE;
  }
  return TreeNodeType.TABLE;
};

export const getDatabaseObjectTypeByTreeNodeType = (
  treeNodeType: TreeNodeType,
): EditorTableIdentifier['objectType'] => {
  if (treeNodeType === TreeNodeType.VIEW) {
    return 'VIEW';
  }
  if (treeNodeType === TreeNodeType.FUNCTION) {
    return 'FUNCTION';
  }
  if (treeNodeType === TreeNodeType.PROCEDURE) {
    return 'PROCEDURE';
  }
  return 'TABLE';
};

export const getParentTreeNodeTypeByObjectType = (objectType: EditorTableIdentifier['objectType']) => {
  if (objectType === 'VIEW') {
    return TreeNodeType.VIEWS;
  }
  if (objectType === 'FUNCTION') {
    return TreeNodeType.FUNCTIONS;
  }
  if (objectType === 'PROCEDURE') {
    return TreeNodeType.PROCEDURES;
  }
  return TreeNodeType.TABLES;
};

export const createDatabaseObjectTreeNodeKey = (params: DatabaseObjectTreeNodeMatchParams) => {
  const { treeNodeType, dataSourceId, databaseName, schemaName, name } = params;
  const normalizedDatabaseName = normalizeTreeNodeKeyPart(databaseName);
  const normalizedSchemaName = normalizeTreeNodeKeyPart(schemaName);

  if (treeNodeType === TreeNodeType.TABLE) {
    return [
      `dataSource_${dataSourceId}`,
      normalizedDatabaseName ? `database_${normalizedDatabaseName}` : '',
      normalizedSchemaName ? `schema_${normalizedSchemaName}` : '',
      `table_${name}`,
    ].join('-');
  }

  if (treeNodeType === TreeNodeType.VIEW) {
    return [
      `dataSource_${dataSourceId}`,
      `database_${normalizedDatabaseName}`,
      `schema_${normalizedSchemaName}`,
      `view_${name}`,
    ].join('-');
  }

  return null;
};

export const findDatabaseObjectTreeNode = (
  treeData: TreeNodeData[] | null,
  params: DatabaseObjectTreeNodeMatchParams,
) => {
  const tableKey = createDatabaseObjectTreeNodeKey(params);

  return findTreeNodeByKey(tableKey, treeData) || findTreeNodeByDetail(treeData, params);
};

export const findTreeNodeByKey = (
  key: Key | null | undefined,
  treeData: TreeNodeData[] | null,
): TreeNodeData | null => {
  if (!key || !treeData) {
    return null;
  }

  for (const node of treeData) {
    if (node.key === key) {
      return node;
    }

    const childNode = findTreeNodeByKey(key, node.children || null);
    if (childNode) {
      return childNode;
    }
  }

  return null;
};

export const findTreeNodeByDetail = (
  treeData: TreeNodeData[] | null,
  params: DatabaseObjectTreeNodeMatchParams,
): TreeNodeData | null => {
  const { treeNodeType, dataSourceId, databaseName, schemaName, name } = params;
  if (!name) {
    return null;
  }

  return findTreeNode(
    treeData,
    (node) =>
      node.treeNodeType === treeNodeType &&
      node.extraParams?.dataSourceId === dataSourceId &&
      isSameTreeNodeKeyPart(node.extraParams?.databaseName, databaseName, params.databaseType) &&
      isSameTreeNodeKeyPart(node.extraParams?.schemaName, schemaName, params.databaseType) &&
      isSameTreeNodeKeyPart(getTreeNodeObjectName(node), name, params.databaseType),
  );
};

export const findTreeNode = (
  treeData: TreeNodeData[] | null,
  predicate: (node: TreeNodeData) => boolean,
): TreeNodeData | null => {
  if (!treeData) {
    return null;
  }

  for (const node of treeData) {
    if (predicate(node)) {
      return node;
    }

    const childNode = findTreeNode(node.children || null, predicate);
    if (childNode) {
      return childNode;
    }
  }

  return null;
};

export const getTreeNodeObjectName = (node: TreeNodeData) => {
  if (node.treeNodeType === TreeNodeType.FUNCTION) {
    return node.extraParams?.functionName;
  }
  if (node.treeNodeType === TreeNodeType.PROCEDURE) {
    return node.extraParams?.procedureName;
  }
  if (node.treeNodeType === TreeNodeType.VIEW) {
    return node.extraParams?.viewName;
  }
  return node.extraParams?.tableName;
};

function normalizeTreeNodeKeyPart(value?: string | null) {
  return value === '' || value === null ? undefined : value;
}

function isSameTreeNodeKeyPart(
  left?: string | null,
  right?: string | null,
  databaseType?: EditorTableIdentifier['databaseType'],
) {
  return normalizeTreeNodeMatchValue(left, databaseType) === normalizeTreeNodeMatchValue(right, databaseType);
}

function normalizeTreeNodeMatchValue(
  value?: string | null,
  databaseType?: EditorTableIdentifier['databaseType'],
) {
  const normalizedValue = normalizeTreeNodeKeyPart(value);
  if (normalizedValue === undefined) {
    return undefined;
  }

  if (isOracleLikeDatabase(databaseType)) {
    return normalizedValue.toUpperCase();
  }

  return normalizedValue.toLowerCase();
}

function isOracleLikeDatabase(databaseType?: EditorTableIdentifier['databaseType']) {
  return databaseType === DatabaseTypeCode.ORACLE || databaseType === DatabaseTypeCode.OCEANBASE_ORACLE;
}

export const formatTableString = (tableStr: string) => {
  let res = '';
  let tableObj;
  try {
    tableObj = JSON.parse(tableStr);
  } catch (error) {
    console.error('Failed to parse table string:', error);
    // Return default value or throw error depending on your needs
    tableObj = {}; // Or use other default values
    // Or you can re-throw the error
    // throw new Error(`Failed to parse table string: ${error.message}`);
  }
  for (const key of Object.keys(tableObj)) {
    const table = tableObj[key];
    res += formatTableSchemaToMarkdown(table.tableName, table.tableSchema);
  }
  return { tableStr: res, tableObj };
};

export const formatTableSchemaToMarkdown = (tableName: string, tableSchema?: string) => {
  if (!tableSchema) {
    return '';
  }
  const schema = tableSchema.replaceAll(/`(.*?)`/g, '{{$1}}').replaceAll(/\n/g, '\\n');
  return `\`table::${tableName}::${schema}\` `;
};

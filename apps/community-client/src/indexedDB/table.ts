
export interface IWorkspaceConsoleDDL {
  consoleId: string; // The id of the console is unique
  ddl: string; // datasourceddl
  userId?: string; // User's unique id
}

// Workspace console table
export const workspaceConsoleDDL = {
  name: 'workspaceConsoleDDL',
  primaryKey: {
    keyPath: 'consoleId',
    autoIncrement: true,
  },
  column: [
    {
      name: 'consoleId',
      isIndex: true,
      keyPath: 'consoleId',
      options: {
        unique: true,
      },
    },
    {
      name: 'userId',
      isIndex: true,
      keyPath: 'userId',
      options: {
        unique: false,
      },
    },
    {
      name: 'ddl',
      isIndex: true,
      keyPath: 'ddl',
      options: {
        unique: false,
      },
    },

  ],
}

export const tableList = [
  {
    tableDetails: workspaceConsoleDDL,
  }
]

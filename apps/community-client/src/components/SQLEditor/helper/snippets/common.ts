const commonSnippets = {
  sel: {
    snippet: 'SELECT \n\t${2:*} \nFROM \n\t${1:};\n',
    detail: 'Select data from a table',
  },
  selc: {
    snippet: 'SELECT \n\tcount(*) \nFROM \n\t${1:} \nWHERE \n\t${2:};\n',
    detail: 'Select data from a table with a condition',
  },
  ins: {
    snippet: 'INSERT INTO \n\t${1:} (${2:})\nVALUES (${3:});\n',
    detail: 'Insert data into a table',
  },
  upd: {
    snippet: 'UPDATE \n\t${1:} \nSET \n\t${2:} \nWHERE \n\t${3:};\n',
    detail: 'Update data in a table',
  },
  del: {
    snippet: 'DELETE FROM \n\t${1:} \nWHERE \n\t${2:};\n',
    detail: 'Delete data from a table',
  },
};

export default commonSnippets;

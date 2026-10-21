import { tableList } from './table';

// How to create a database
export const createDB = (dbName: string, version: number) => {
  return new Promise((resolve, reject) => {
    const request = window.indexedDB.open(dbName, version);
    request.onerror = (event: any) => {
      reject(event.target.error);
    };
    request.onsuccess = (event: any) => {
      resolve(event.target.result);
    };
    request.onupgradeneeded = (event: any) => {
      const db = event.target.result; // database object
      // Create repository
      tableList.forEach((item: any) => {
        const { tableDetails } = item;
        const objectStore = db.createObjectStore(tableDetails.name, tableDetails.primaryKey);
        tableDetails.column.forEach((i: any) => {
          if (i.isIndex) {
            objectStore.createIndex(i.name, i.keyPath, i.options);
          }
        });
      });
    };
  });
};

type TableType = 'workspaceConsoleDDL';

type DBType = 'chat2db';

// Add data
export const addData = (db: DBType, tableName: TableType, data: any) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.add(data);
    request.onsuccess = () => {
      resolve(true);
    };
    request.onerror = (error) => {
      reject(error);
    };
  });
};

// Delete data by index
export const deleteDataByIndex = (db: DBType, tableName: TableType, indexName, indexValue) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.index(indexName).delete(indexValue);
    request.onsuccess = () => {
      resolve(true);
    };
    request.onerror = () => {
      reject(false);
    };
  });
};

// Delete data by primary key
export const deleteData = (db: DBType, tableName: TableType, key: any) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.delete(key);
    request.onsuccess = () => {
      resolve(true);
    };
    request.onerror = () => {
      reject(false);
    };
  });
};

// Query data through indexes, supporting multiple indexes
export const getDataByIndex = (db: DBType, tableName: TableType, indexName: string, indexValue: any) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.index(indexName).get(indexValue);
    request.onsuccess = () => {
      resolve(request.result);
    };
    request.onerror = () => {
      reject(false);
    };
  });
};

// Query data through cursor and support passing in multiple conditions
export const getDataByCursor = (db: DBType, tableName: TableType, condition: {[key in string]: any}
) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.openCursor();
    const result: any[] = [];
    request.onsuccess = (event: any) => {
      const cursor = event.target.result;
      if (cursor) {
        let flag = true;
        Object.keys(condition).forEach((key) => {
          if (cursor.value[key] !== condition[key]) {
            flag = false;
          }
        });
        if (flag) {
          result.push(cursor.value);
        }
        cursor.continue();
      } else {
        resolve(result);
      }
    };
    request.onerror = () => {
      reject(false);
    };
  });
 
};


// Modify data
export const updateData = (db: DBType, tableName: TableType, data: any) => {
  return new Promise((resolve, reject) => {
    const transaction = window._indexedDB[db].transaction(tableName, 'readwrite');
    const objectStore = transaction.objectStore(tableName);
    const request = objectStore.put(data);
    request.onsuccess = () => {
      resolve(true);
    };
    request.onerror = () => {
      reject(false);
    };
  });
};

// Close database
export const closeDB = (db: DBType) => {
  return new Promise((resolve) => {
    window._indexedDB[db].close();
    resolve(true);
  });
};

export default {
  createDB,
  addData,
  deleteDataByIndex,
  deleteData,
  getDataByIndex,
  getDataByCursor,
  updateData,
  closeDB,
};

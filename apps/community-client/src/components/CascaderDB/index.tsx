import React, { useEffect, useState } from 'react';
import { Divider, Select, Spin, Typography } from 'antd';
import connection from '@/service/connection';
import cs from 'classnames';
import Iconfont from '../Iconfont';
import { databaseMap } from '@/constants/database';
import sqlService from '@/service/sql';
import styles from './index.less';

interface IOption {
  key?: string;
  label: string | React.ReactNode;
  value: number | string;
}

type IDBItemType = 'datasource' | 'schema' | 'database' | 'table' | 'column';
interface IDataCascader {
  dataSourceId: number;
  databaseName: string;
  schemaName?: string;
  tableName?: string;
  columnNames?: string[];
}

interface IProps {
  className?: string;
  dataSourceId?: number;
  dataSourceOptions?: IOption[];
  disabled?: Array<IDBItemType>;
  notShowList?: Array<IDBItemType>;
  initData?: Partial<IDataCascader>;
  onChange?: (value: IDataCascader) => void;
}

function CascaderDB(props: IProps) {
  const { notShowList = [], initData } = props;
  const [dataSourceOptions, setDataSourceOptions] = useState<IOption[]>(props?.dataSourceOptions || []);
  const [curDataSourceId, setCurDataSourceId] = useState<number | undefined>();

  const [databaseOptions, setDatabaseOptions] = useState<IOption[]>([]);
  const [curDatabaseName, setCurDatabaseName] = useState<string>('');

  const [schemaOptions, setSchemaOptions] = useState<IOption[]>([]);
  const [curSchemeName, setCurSchemeName] = useState<string>('');

  const [tableOptions, setTableOptions] = useState<IOption[]>([]);
  const [curTableName, setCurTableName] = useState<string>('');

  const [columnOptions, setColumnOptions] = useState<IOption[]>([]);
  const [curColumnList, setCurColumnList] = useState<string[]>([]);

  const [isDataSourceLoading, setIsDataSourceLoading] = useState<boolean>(false);
  const [isDatabaseLoading, setIsDatabaseLoading] = useState<boolean>(false);
  const [isSchemaLoading, setIsSchemaLoading] = useState<boolean>(false);
  const [isTableLoading] = useState<boolean>(false);
  const [isColumnLoading] = useState<boolean>(false);

  useEffect(() => {
    if (initData && Object.keys(initData).length) {
      const { dataSourceId, databaseName, schemaName, tableName, columnNames } = initData;
      setCurDataSourceId(dataSourceId ? Number(dataSourceId) : undefined);
      setCurDatabaseName(databaseName || '');
      setCurSchemeName(schemaName || '');
      setCurTableName(tableName || '');
      setCurColumnList(columnNames || []);
    }

    if (!props.dataSourceOptions) {
      loadDataSource();
    }
  }, []);

  useEffect(() => {
    if (props.dataSourceId) {
      setCurDataSourceId(props.dataSourceId);
    }
  }, [props.dataSourceId]);

  useEffect(() => {
    loadDatabase();
  }, [curDataSourceId]);

  useEffect(() => {
    if (databaseOptions.length && !curDatabaseName) {
      return;
    }

    if (schemaOptions.length && !curSchemeName) {
      return;
    }

    if (!curDatabaseName && !curSchemeName) {
      return;
    }
    loadTable();
  }, [curDatabaseName, curSchemeName]);

  useEffect(() => {
    if (curTableName) {
      loadColumn();
    }
  }, [curTableName]);

  const handleChangeDataSource = (value) => {
    setCurDataSourceId(value);
    setDatabaseOptions([]);
    setSchemaOptions([]);
    setTableOptions([]);
    setCurDatabaseName('');
    setCurSchemeName('');
    setCurTableName('');

    props.onChange &&
      props.onChange({
        dataSourceId: value,
        databaseName: '',
        schemaName: '',
        tableName: '',
      });
  };
  const handleChangeDatabase = (value) => {
    setCurDatabaseName(value);
    setSchemaOptions([]);
    setCurSchemeName('');

    props.onChange &&
      props.onChange({
        dataSourceId: curDataSourceId!,
        databaseName: value,
        schemaName: '',
        tableName: '',
      });
  };

  const handleChangeSchema = (value) => {
    setCurSchemeName(value);

    props.onChange &&
      props.onChange({
        dataSourceId: curDataSourceId!,
        databaseName: curDatabaseName,
        schemaName: value,
        tableName: '',
      });
  };
  const handleChangeTable = (value) => {
    setCurTableName(value);
    setCurColumnList(['all']);

    props.onChange &&
      props.onChange({
        dataSourceId: curDataSourceId!,
        databaseName: curDatabaseName,
        schemaName: curSchemeName,
        tableName: value,
      });
  };
  const handleChangeColumn = (value: string[]) => {
    let columnListTemp = [...value];
    // "All" is mutually exclusive with other options.
    if (curColumnList.includes('all')) {
      columnListTemp = value.filter((item) => item !== 'all');
    } else {
      if (value.includes('all')) {
        columnListTemp = ['all'];
      }
    }
    setCurColumnList(columnListTemp);

    props.onChange &&
      props.onChange({
        dataSourceId: curDataSourceId!,
        databaseName: curDatabaseName,
        schemaName: curSchemeName,
        tableName: curTableName,
        columnNames: columnListTemp,
      });
  };
  const handleOnChange = (
    dataSourceId: number,
    databaseName: string,
    schemaName: string,
    tableName: string,
    columnNames: string[],
  ) => {
    props.onChange &&
      props.onChange({
        dataSourceId,
        databaseName,
        schemaName,
        tableName,
        columnNames,
      });
  };

  /** Load data-source data. */
  const loadDataSource = async () => {
    // Request data-source data.
    setIsDataSourceLoading(true);
    try {
      const dataSourceList = await connection.getList({
        pageNo: 1,
        pageSize: 999,
        refresh: true,
      });
      const formattedData = (dataSourceList?.data || []).map((item) => ({
        ...item,
        key: `dataSource-${item.id}`,
        value: item.id,
        label: (
          <div className={styles.optionItem}>
            <Iconfont className={styles.optionItemIcon} code={databaseMap[item.type]?.icon} />
            <div className={styles.optionItemText}>{item.alias}</div>
          </div>
        ),
      }));
      setDataSourceOptions(formattedData);
      if (!curDataSourceId && !initData?.dataSourceId) {
        setCurDataSourceId(formattedData[0]?.value);
        handleOnChange(formattedData[0]?.value, '', '', '', []);
      }
    } catch (error) {
      console.error('get dataSourceList error', error);
    } finally {
      setIsDataSourceLoading(false);
    }
  };

  /** Load database data. */
  const loadDatabase = async () => {
    if (curDataSourceId === undefined) {
      return;
    }

    setIsDatabaseLoading(true);
    try {
      const databaseList = await connection.getDatabaseList({
        dataSourceId: curDataSourceId,
      });

      const formattedData = (databaseList || []).map((item) => ({
        ...item,
        key: `database-${item.name}`,
        value: item.name,
        label: (
          <div className={styles.optionItem}>
            <Iconfont className={styles.optionItemIcon} code="&#xe744;" />
            <div className={styles.optionItemText}>{item.name}</div>
          </div>
        ),
      }));

      setDatabaseOptions(formattedData);
      loadSchema(formattedData[0]?.value);
      if (!curDatabaseName && !initData?.databaseName && formattedData[0]?.value) {
        setCurDatabaseName(formattedData[0]?.value);
        handleOnChange(curDataSourceId, formattedData[0]?.value, '', '', []);
      }
    } catch (error) {
      console.log('get databaseList error', error);
    } finally {
      setIsDatabaseLoading(false);
    }
  };

  const loadSchema = async (databaseName: string) => {
    if (curDataSourceId === undefined) {
      return;
    }
    setIsSchemaLoading(true);
    try {
      const schemaList = await connection.getSchemaList({
        dataSourceId: curDataSourceId,
        databaseName,
        refresh: false,
      });

      const formattedData = (schemaList || []).map((item) => ({
        ...item,
        key: `schema-${item.name}`,
        value: item.name,
        label: (
          <div className={styles.optionItem}>
            <Iconfont className={styles.optionItemIcon} code="&#xe696;" />
            <div className={styles.optionItemText}>{item.name}</div>
          </div>
        ),
      }));

      setSchemaOptions(formattedData);
      if (!curSchemeName && !initData?.schemaName && formattedData[0]?.value) {
        setCurSchemeName(formattedData[0]?.value);
        handleOnChange(curDataSourceId, databaseName, formattedData[0]?.value, '', []);
      }
    } catch (error) {
      console.log('get schemaList error', error);
    } finally {
      setIsSchemaLoading(false);
    }
  };

  const loadTable = async () => {
    if (
      curDataSourceId === undefined ||
      (!curDatabaseName && !curSchemeName) ||
      // Some contexts do not need to show tables.
      props?.notShowList?.includes('table')
    ) {
      return;
    }

    const tableList = await sqlService.getTableList({
      dataSourceId: curDataSourceId,
      databaseName: curDatabaseName,
      schemaName: curSchemeName,
      pageNo: 1,
      pageSize: 100,
    });

    const formattedData = (tableList?.data || []).map((item) => ({
      // ...item,
      key: `table-${item.name}`,
      value: item.name,
      label: (
        <div className={styles.optionItem}>
          <Iconfont className={styles.optionItemIcon} code="&#xe618;" />
          <div className={styles.optionItemText}>{item.name}</div>
        </div>
      ),
    }));

    setTableOptions(formattedData);
    if (!curTableName && !initData?.tableName && formattedData[0]?.value) {
      setCurTableName(formattedData[0]?.value);
      handleOnChange(curDataSourceId, curDatabaseName, curSchemeName, formattedData[0]?.value, []);
    }
  };

  const loadColumn = async () => {
    if (
      !curDataSourceId ||
      (!curDatabaseName && !curSchemeName) ||
      !curTableName ||
      // Some contexts do not need to show columns.
      props?.notShowList?.includes('column')
    )
      return;

    const columnResult = await sqlService.getColumnList({
      dataSourceId: curDataSourceId!,
      databaseName: curDatabaseName,
      schemaName: curSchemeName,
      tableName: curTableName,
    });

    const formattedData = [
      {
        value: 'all',
        label: (
          <div className={styles.optionItem}>
            <Iconfont className={styles.optionItemIcon} code="&#xe696;" />
            <div className={styles.optionItemText}>全部</div>
          </div>
        ),
      },
      ...(columnResult.data || []).map((item) => ({
        ...item,
        key: `column-${item.name}`,
        value: item.name,
        label: (
          <div className={styles.optionItem}>
            <Iconfont className={styles.optionItemIcon} code="&#xe696;" />
            <div className={styles.optionItemText}>{item.name}</div>
          </div>
        ),
      })),
    ];
    setColumnOptions(formattedData);
    if (!curColumnList.length && !initData?.columnNames && formattedData[0]?.value) {
      setCurColumnList([formattedData[0]?.value]);
      handleOnChange(curDataSourceId, curDatabaseName, curSchemeName, curTableName, [formattedData[0]?.value]);
    }
  };

  return (
    <div className={cs(props.className, styles.cascaderDB)}>
      {notShowList?.includes('datasource') ? null : (
        <Spin spinning={isDataSourceLoading}>
          <Select
            disabled={props.disabled?.includes('datasource')}
            bordered={false}
            placeholder="请选择连接"
            showSearch
            popupMatchSelectWidth={false}
            options={dataSourceOptions}
            value={curDataSourceId}
            onChange={handleChangeDataSource}
            className={styles.select}
            dropdownRender={(menu) => (
              <>
                <div style={{ padding: '8px 16px' }}>
                  <Typography.Title level={4}>连接源</Typography.Title>
                  <Divider style={{ margin: '4px 0' }} />
                </div>
                {menu}
              </>
            )}
          />
        </Spin>
      )}

      {notShowList?.includes('database') ? null : (
        <Spin spinning={isDatabaseLoading}>
          {!!databaseOptions.length && (
            <Select
              disabled={props.disabled?.includes('database')}
              bordered={false}
              placeholder="请选择数据库"
              showSearch
              popupMatchSelectWidth={false}
              options={databaseOptions}
              value={curDatabaseName}
              onChange={handleChangeDatabase}
              className={styles.select}
              dropdownRender={(menu) => (
                <>
                  <div style={{ padding: '8px 16px' }}>
                    <Typography.Title level={4}>数据库</Typography.Title>
                    <Divider style={{ margin: '4px 0' }} />
                  </div>
                  {menu}
                </>
              )}
            />
          )}
          {isDatabaseLoading && <div style={{ width: '100px' }} />}
        </Spin>
      )}

      {notShowList?.includes('schema') ? null : (
        <Spin spinning={isSchemaLoading}>
          {!!schemaOptions.length && (
            <Select
              disabled={props.disabled?.includes('schema')}
              bordered={false}
              placeholder="请选择Schema"
              showSearch
              popupMatchSelectWidth={false}
              options={schemaOptions}
              value={curSchemeName}
              onChange={handleChangeSchema}
              className={styles.select}
              dropdownRender={(menu) => (
                <>
                  <div style={{ padding: '8px 16px' }}>
                    <Typography.Title level={4}>Schema</Typography.Title>
                    <Divider style={{ margin: '4px 0' }} />
                  </div>
                  {menu}
                </>
              )}
            />
          )}
          {isSchemaLoading && <div style={{ width: '100px' }} />}
        </Spin>
      )}

      {notShowList?.includes('table') ? null : (
        <Spin spinning={isTableLoading}>
          {!!tableOptions.length && (
            <Select
              disabled={props.disabled?.includes('table')}
              bordered={false}
              placeholder="请选择Table"
              showSearch
              popupMatchSelectWidth={false}
              options={tableOptions}
              value={curTableName}
              onChange={handleChangeTable}
              className={styles.select}
              dropdownRender={(menu) => (
                <>
                  <div style={{ padding: '8px 16px' }}>
                    <Typography.Title level={4}>Table</Typography.Title>
                    <Divider style={{ margin: '4px 0' }} />
                  </div>
                  {menu}
                </>
              )}
            />
          )}
          {isTableLoading && <div style={{ width: '100px' }} />}
        </Spin>
      )}

      {notShowList?.includes('column') ? null : (
        <Spin spinning={isTableLoading}>
          {!!columnOptions.length && (
            <Select
              disabled={props.disabled?.includes('column')}
              mode="multiple"
              bordered={false}
              placeholder="请选择Column"
              showSearch
              popupMatchSelectWidth={false}
              options={columnOptions}
              value={curColumnList}
              onChange={handleChangeColumn}
              className={styles.select}
              dropdownRender={(menu) => (
                <>
                  <div style={{ padding: '8px 16px' }}>
                    <Typography.Title level={4}>Column</Typography.Title>
                    <Divider style={{ margin: '4px 0' }} />
                  </div>
                  {menu}
                </>
              )}
            />
          )}
          {isColumnLoading && <div style={{ width: '100px' }} />}
        </Spin>
      )}
    </div>
  );
}

export default CascaderDB;

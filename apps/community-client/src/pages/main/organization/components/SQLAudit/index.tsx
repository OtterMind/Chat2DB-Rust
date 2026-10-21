import { useEffect, useMemo, useState } from 'react';
import historyService, { IHistoryRecord, OperationTypeEnum } from '@/service/history';
import { Button } from 'antd';
import i18n from '@/i18n';
import { useStyles } from './style';
import PageTitle from '@/components/PageTitle';
import { IconfontSvg } from '@chat2db/ui';
import AntdTable from '@/components/AntdTable';
/**
 historyService
      .getHistoryList({
        // dataSourceId:props.curWorkspaceParams.dataSourceId,
        pageNo: curPageRef.current++,
        pageSize: 40,
        operationType: OperationTypeEnum.SQL_EXECUTE,
      })
 */

const initPagination = {
  searchKey: '',
  current: 1,
  pageSize: 10,
  total: 0,
};

const SQLAudit = () => {
  const [dataSource, setDataSource] = useState<IHistoryRecord[]>([]);
  const [pagination, setPagination] = useState(initPagination);

  const { styles } = useStyles();

  const columns = useMemo(
    () => [
      {
        title: i18n('team.sqlAudit.table.sql'),
        dataIndex: 'ddl',
        key: 'ddl',
      },
      {
        title: i18n('team.sqlAudit.table.time'),
        dataIndex: 'gmtCreate',
        key: 'gmtCreate',
        width: 200,
      },
      {
        title: i18n('team.sqlAudit.table.user'),
        dataIndex: 'userName',
        key: 'userName',
        width: 160,
      },
    ],
    [],
  );

  useEffect(() => {
    queryHistoryList();
  }, [pagination.searchKey, pagination.current, pagination.pageSize]);

  const queryHistoryList = async () => {
    const res = await historyService.getHistoryList({
      pageNo: pagination.current,
      pageSize: pagination.pageSize,
      searchKey: pagination.searchKey,
      operationType: OperationTypeEnum.SQL_AUDIT,
    });

    if (res) {
      setDataSource(res?.data ?? []);
      setPagination({
        ...pagination,
        total: res.total,
      });
    }
  };

  const handleTableChange = (p) => {
    setPagination({
      ...pagination,
      ...p,
    });
  };

  return (
    <div className={styles.wrapper}>
      <PageTitle title={i18n('team.nav.sqlAudit')} />
      <div className={styles.tableTop}>
        {/* <Input.Search
          style={{ width: '320px' }}
          placeholder={i18n('common.text.searchPlaceholder')}
          onSearch={handleSearch}
          enterButton={<SearchOutlined />}
        /> */}

        <div />

        <Button
          type="primary"
          icon={<IconfontSvg code={'icon-refresh'} size="sm" />}
          onClick={() => {
            queryHistoryList();
          }}
        >
          {i18n('common.button.refresh')}
        </Button>
      </div>
      <AntdTable
        className={styles.antdTable}
        rowKey={'id'}
        dataSource={dataSource}
        columns={columns}
        pagination={pagination}
        onChange={handleTableChange}
      />
    </div>
  );
};

export default SQLAudit;

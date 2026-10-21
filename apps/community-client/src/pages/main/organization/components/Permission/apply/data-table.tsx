import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from 'react';
import { ApplyType, IAccessControlApplyRecordVO } from '@/typings/enterprise/permission';
import { Button, Tag } from 'antd';
import permissionService from '@/service/enterprise/permission';
import i18n from '@/i18n';
import styles from './index.less';
import { useOrgStore } from '@/store/organization';
import { ApprovalStatusType } from '@/typings/enterprise/approval';
import AntdTable from '@/components/AntdTable';

interface IProps {
  onClickAdd: () => void;
  onClickDetail: (record: IAccessControlApplyRecordVO) => void;
}

const DataTable = forwardRef((props: IProps, ref) => {
  const [dataSource, setDataSource] = useState<IAccessControlApplyRecordVO[]>([]);
  const [pagination, setPagination] = useState({
    searchKey: '',
    current: 1,
    pageSize: 10,
    total: 0,
  });
  const columns = useMemo(
    () => [
      {
        title: i18n('team.apply.table.name'),
        dataIndex: 'name',
        key: 'name',
      },
      {
        title: i18n('team.apply.table.desc'),
        dataIndex: 'description',
        key: 'description',
      },
      {
        title: i18n('team.apply.table.status'),
        dataIndex: 'status',
        key: 'status',
        render: (text: string) => {
          if (text === ApprovalStatusType.APPROVED) {
            return <Tag color="green">{i18n('team.approval.status.approved')}</Tag>;
          } else if (text === ApprovalStatusType.REJECTED) {
            return <Tag color="red">{i18n('team.approval.status.reject')}</Tag>;
          } else if (text === ApprovalStatusType.PENDING) {
            return <Tag color="blue">{i18n('team.approval.status.pending')}</Tag>;
          }
        },
      },
      {
        title: i18n('team.apply.table.action'),
        dataIndex: 'action',
        key: 'action',
        width: 160,
        render: (_: any, record: IAccessControlApplyRecordVO) => (
          <>
            <Button
              type="link"
              onClick={() => {
                console.log('detail', record);
                props.onClickDetail && props.onClickDetail(record);
              }}
            >
              {i18n('team.approval.action.view')}
            </Button>
          </>
        ),
      },
    ],
    [],
  );
  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));

  useImperativeHandle(ref, () => ({
    queryTableList: queryDataTableList,
  }));

  useEffect(() => {
    queryDataTableList();
  }, [pagination.searchKey, pagination.current, pagination.pageSize]);

  const queryDataTableList = async () => {
    if (!curOrg?.id) return;
    const { searchKey, current, pageSize } = pagination;
    const res = await permissionService.queryApplyList({
      organizationId: curOrg?.id,
      applyType: ApplyType.OPERATOR,
      searchKey,
      pageNo: current,
      pageSize,
    });
    setDataSource(res.data || []);
    setPagination({
      ...pagination,
      total: res.total,
    });
  };

  const handleTableChange = (p) => {
    setPagination({
      ...pagination,
      ...p,
    });
  };

  return (
    <div className={styles.block}>
      {/* <div className={styles.title}>Data permissions</div> */}
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
          onClick={() => {
            props.onClickAdd && props.onClickAdd();
          }}
        >
          {i18n('team.apply.applyData')}
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
});

export default DataTable;

import { useEffect, useMemo, useState } from 'react';
import { ApprovalStatusMap, ApprovalStatusType, IApprovalProcessVO } from '@/typings/enterprise/approval';
import { Button, Drawer, Segmented, Tag } from 'antd';
import approvalService from '@/service/enterprise/approval';
import styles from './index.less';
import ApprovalFlow from './approvalFlow';
import { useOrgStore } from '@/store/organization';
import PageTitle from '@/components/PageTitle';
import i18n from '@/i18n';
import AntdTable from '@/components/AntdTable';

// Select several types as filter criteria.
const selectedStatusMap = {
  [ApprovalStatusType.PENDING]: ApprovalStatusMap.PENDING,
  [ApprovalStatusType.APPROVED]: ApprovalStatusMap.APPROVED,
  [ApprovalStatusType.REJECTED]: ApprovalStatusMap.REJECTED,
};

// Colors.
const colorStatus = {
  [ApprovalStatusType.PENDING]: 'blue',
  [ApprovalStatusType.APPROVED]: 'green',
  [ApprovalStatusType.REJECTED]: 'red',
};

const segmentOptions = Object.keys(selectedStatusMap).map((key) => ({
  label: ApprovalStatusMap[key],
  value: key,
}));

function Approve() {
  const [loading, setLoading] = useState(false);
  const [dataSource, setDataSource] = useState<IApprovalProcessVO[]>([]);
  const [pagination, setPagination] = useState({
    searchKey: '',
    approvalStatus: '',
    current: 1,
    pageSize: 10,
    showSizeChanger: true,
    showQuickJumper: true,
    total: 0,
    // pageSizeOptions: ['10', '20', '30', '40'],
  });
  const [curApproval, setCurApproval] = useState<IApprovalProcessVO>();
  const [open, setOpen] = useState(false);

  const { curOrg } = useOrgStore((state) => ({
    curOrg: state.curOrg,
  }));

  const columns = useMemo(
    () => [
      {
        title: i18n('team.approval.table.id'),
        dataIndex: 'id',
        key: 'id',
      },
      {
        title: i18n('team.approval.table.name'),
        dataIndex: 'name',
        key: 'name',
      },

      {
        title: i18n('team.approval.table.status'),
        dataIndex: 'approvalStatus',
        key: 'approvalStatus',
        render: (status: ApprovalStatusType) => <Tag color={colorStatus[status]}>{ApprovalStatusMap[status]}</Tag>,
      },
      {
        title: i18n('team.approval.table.action'),
        key: 'action',
        width: 120,
        render: (_: any, record: IApprovalProcessVO) => (
          <>
            <Button
              type="link"
              onClick={() => {
                setCurApproval(record);
                setOpen(true);
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

  useEffect(() => {
    queryApprovalList();
  }, [pagination.approvalStatus, pagination.pageSize, pagination.searchKey, pagination.current]);

  const queryApprovalList = async () => {
    setLoading(true);
    try {
      const { searchKey, current: pageNo, pageSize, approvalStatus } = pagination;
      const res = await approvalService.queryApprovalList({
        searchKey,
        pageNo,
        pageSize,
        organizationId: curOrg?.id as number,
        approvalStatus: approvalStatus as ApprovalStatusType,
      });
      setDataSource(res?.data);
      setPagination({
        ...pagination,
        total: res?.total,
      });
    } catch {
      setDataSource([]);
    } finally {
      setLoading(false);
    }
  };

  const handleFliterChange = (approvalStatus: ApprovalStatusType) => {
    setPagination({
      ...pagination,
      approvalStatus,
    });
  };

  const handleTableChange = (p: any) => {
    setPagination({
      ...pagination,
      ...p,
    });
  };

  const handleCancelDrawer = () => {
    setOpen(false);
    setCurApproval(undefined);
  };

  return (
    <div className={styles.wrapper}>
      <PageTitle title={i18n('team.nav.approval')} />
      <div className={styles.tableTop}>
        {/* <Input.Search
          style={{ width: '320px' }}
          // placeholder="Enter keywords to search"
          onSearch={handleSearch}
          enterButton={<SearchOutlined />}
        /> */}
        <div />
        <Segmented
          options={[{ label: i18n('team.approval.status.all'), value: ' ' }, ...segmentOptions]}
          onChange={handleFliterChange}
        />
      </div>
      <AntdTable
        className={styles.antdTable}
        rowKey={'id'}
        loading={loading}
        dataSource={dataSource}
        columns={columns}
        pagination={pagination}
        onChange={handleTableChange}
      />
      <Drawer
        width={'720px'}
        title={<span>{i18n('team.approval.flow.title')}</span>}
        open={open}
        onClose={handleCancelDrawer}
      >
        <ApprovalFlow
          onRefresh={() => {
            queryApprovalList();
          }}
          approvalId={curApproval?.rootId}
        />
      </Drawer>
    </div>
  );
}

export default Approve;

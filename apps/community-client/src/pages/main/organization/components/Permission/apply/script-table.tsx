import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from 'react';
import { Button, Input, Table, Tag } from 'antd';
import { SearchOutlined } from '@ant-design/icons';
import { IAccessControlApplyRecordVO } from '@/typings/enterprise/permission';
import { useOrgStore } from '@/store/organization';
import i18n from '@/i18n';
import permissionService from '@/service/enterprise/permission';
import { BooleanType, IPageParams } from '@/typings/common';
import dayjs from 'dayjs';
import styles from './index.less';

interface IProps {
  onClickAdd: () => void;
  onClickDetail: (record: IAccessControlApplyRecordVO) => void;
}

const ScriptTable = forwardRef((props: IProps, ref) => {
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
        title: '申请名称',
        dataIndex: 'name',
        key: 'name',
      },
      {
        title: '申请描述',
        dataIndex: 'description',
        key: 'description',
      },
      {
        title: '状态',
        dataIndex: 'status',
        key: 'status',
        render: (text: string) => {
          if (text === 'APPROVED') {
            return <Tag color="green">{'已通过'}</Tag>;
          } else if (text === 'REJECTED') {
            return <Tag color="red">{'已拒绝'}</Tag>;
          } else if (text === 'PENDING') {
            return <Tag color="blue">{'待审批'}</Tag>;
          }
        },
      },
      {
        title: '有效期',
        dataIndex: 'valid',
        key: 'valid',
        render: (_: any, record: IAccessControlApplyRecordVO) => {
          if (record.noExpire === BooleanType.Yes) {
            return <span>永久有效</span>;
          }
          return <span>{record?.validUntil ? dayjs(record?.validUntil).format('YYYY-MM-DD') : null}</span>;
        },
      },
      {
        title: '操作',
        dataIndex: 'action',
        key: 'action',
        render: (_: any, record: IAccessControlApplyRecordVO) => (
          <>
            <Button
              type="link"
              onClick={() => {
                console.log('record', record);
                props.onClickDetail && props.onClickDetail(record);
              }}
            >
              详情
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
    queryTableList: queryScriptApplyList,
  }));

  useEffect(() => {
    queryScriptApplyList();
  }, [pagination.searchKey, pagination.current, pagination.pageSize]);

  const queryScriptApplyList = async () => {
    if (!curOrg?.id) return;

    const pageParams: IPageParams = {
      ...pagination,
      pageNo: pagination.current,
    };
    const res = await permissionService.queryApplyList({
      organizationId: curOrg?.id,
      applyType: 'SCRIPT',
      ...pageParams,
    });
    setDataSource(res.data || []);
    setPagination({
      ...pagination,
      total: res.total,
    });
  };

  const handleSearch = (searchKey: string) => {
    setPagination({
      ...pagination,
      searchKey,
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
      {/* <div className={styles.title}>Script permissions</div> */}
      <div className={styles.tableTop}>
        <Input.Search
          style={{ width: '320px' }}
          placeholder={i18n('team.input.search.placeholder')}
          onSearch={handleSearch}
          enterButton={<SearchOutlined />}
        />
        <Button
          type="primary"
          onClick={() => {
            props.onClickAdd && props.onClickAdd();
          }}
        >
          申请脚本权限
        </Button>
      </div>
      <Table
        rowKey={'id'}
        dataSource={dataSource}
        columns={columns}
        pagination={pagination}
        onChange={handleTableChange}
      />
    </div>
  );
});

export default ScriptTable;

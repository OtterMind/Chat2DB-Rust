import { useEffect, useMemo, useState } from 'react';
import { useStyles } from './style';
import PageTitle from '@/components/PageTitle';
import { Tag } from 'antd';
import { useOrgStore } from '@/store/organization';
import { Subscription } from '@/typings/enterprise/user';
import dayjs from 'dayjs';
import { formatCurrency, formatPrice } from '@/utils/price';
import { SubscriptionStatus } from '@/constants/user';
import i18n from '@/i18n';
import AntdTable from '@/components/AntdTable';

const initPagination = {
  searchKey: '',
  current: 1,
  pageSize: 100,
  total: 0,
};

const SubscriptionList = () => {
  const curOrgSubscription = useOrgStore((s) => s.curOrgSubscription);

  const { styles } = useStyles();
  const [dataSource, setDataSource] = useState<Subscription[]>([]);
  const [pagination, setPagination] = useState(initPagination);

  useEffect(() => {
    setDataSource(curOrgSubscription?.items || []);
  }, [curOrgSubscription]);

  const columns = useMemo(
    () => [
      {
        title: i18n('team.subscription.table.no'),
        dataIndex: 'id',
        key: 'id',
      },
      {
        title: i18n('team.subscription.table.count'),
        dataIndex: 'seats',
        render: (seat) => {
          return `${seat}`;
        },
      },
      {
        title: i18n('team.subscription.table.createTime'),
        dataIndex: 'createTime',
        key: 'createTime',
        render: (_, record: Subscription) => {
          return `${dayjs(record.createTime).format('YYYY.MM.DD')}`;
        },
      },
      {
        title: i18n('team.subscription.table.validity'),
        dataIndex: 'validityPeriod',
        key: 'validityPeriod',
        render: (_, record: Subscription) => {
          return `${dayjs(record.startTime).format('YYYY.MM.DD')} ~ ${dayjs(record.endTime).format('YYYY.MM.DD')}`;
        },
      },
      {
        title: i18n('team.subscription.table.status'),
        dataIndex: 'status',
        key: 'status',
        render: (_, { status, startTime, endTime }: Subscription) => {
          const now = dayjs();
          if (status === SubscriptionStatus.Valid) {
            if (now >= dayjs(startTime) && now <= dayjs(endTime)) {
              return <Tag color="success">{i18n('team.subscription.status.using')}</Tag>;
            } else {
              return <Tag color="warning">{i18n('team.subscription.status.notStart')}</Tag>;
            }
          } else {
            return <Tag color="grey">{i18n('team.subscription.status.expired')}</Tag>;
          }
        },
      },
      {
        title: i18n('team.subscription.table.price'),
        dataIndex: 'price',
        key: 'price',
        render: (_, record: Subscription) => {
          const unit = formatCurrency(record.currency);
          return `${unit}${formatPrice(record.price)}`;
        },
      },
    ],
    [],
  );

  const handleTableChange = (p) => {
    setPagination({
      ...pagination,
      ...p,
    });
  };

  return (
    <div className={styles.wrapper}>
      <PageTitle title={i18n('team.nav.subscription')} />
      <div className={styles.tableTop}>
        {/* <Input.Search
          style={{ width: '320px' }}
          placeholder={'Enter a value'}
          onSearch={handleSearch}
          enterButton={<SearchOutlined />}
        /> */}
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

export default SubscriptionList;

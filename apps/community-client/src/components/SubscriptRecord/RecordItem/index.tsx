import { FC, useMemo } from 'react';
import { useStyles } from './style';
import { Subscription } from '@/typings/enterprise/user';
import dayjs from 'dayjs';
import { Button, Flex } from 'antd';
import { SubscriptionType } from '@/constants/user';
import { formatCurrency } from '@/utils/price';

interface IProps {
  item: Subscription;
}

const RecordItem: FC<IProps> = ({ item }) => {
  const { styles } = useStyles();
  const isPersonal = useMemo(() => item.type === SubscriptionType.Personal, [item.type]);
  const unit = useMemo(() => formatCurrency(item.currency), [item.currency]);

  return (
    <div className={styles.wrapper}>
      <div className={styles.header}>
        <span>订单号：{item.id}</span>
        <span>购买时间：{dayjs(item.startTime).format('YYYY.MM.DD')}</span>
      </div>
      <div className={styles.content}>
        <Flex justify="space-between" align="center">
          <Flex align="center" gap={12}>
            <div className={styles.title}>{isPersonal ? '个人套餐' : '团队套餐'}</div>
            {/* TODO:  */}
            <div />
          </Flex>
          <div className={styles.price}>{`${unit}${item.price}`}</div>
        </Flex>
        <Flex justify="space-between" align="center">
          <Flex vertical gap={12}>
            {!isPersonal && (
              <div className={styles.seat}>
                支持<span>{item.seats}</span>位团队成员使用
              </div>
            )}
            <div className={styles.timeDesc}>套餐将于{dayjs(item.endTime).format('YYYY.MM.DD')}过期</div>
          </Flex>
          {!isPersonal && <Button>增加席位</Button>}
        </Flex>
      </div>
    </div>
  );
};

export default RecordItem;

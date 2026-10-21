import React, { memo } from 'react';
import { useStyles } from './style';
import { ProductDetail } from '@/typings/pricing';
import { Flex } from 'antd';

interface IProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  active?: boolean;
  data: ProductDetail;
}

export default memo<IProps>((props) => {
  const { className, active, data, ...rest } = props;
  const { styles, cx } = useStyles({ active, tag: data.tag });
  return (
    <div className={cx(styles.pricingCard, className)} {...rest}>
      {data.tag && <div className={styles.tag}>{data.tag}</div>}
      <Flex vertical justify="start" align="start">
        <div className={styles.productName}>{data.title}</div>
        <Flex align="baseline">
          <div className={styles.productPrice}>
            <span>{data.currencySymbol}</span>
            <span>{data.price}</span>
          </div>
        </Flex>
        <div className={styles.description2}>{data.description2}</div>
      </Flex>
    </div>
  );
});

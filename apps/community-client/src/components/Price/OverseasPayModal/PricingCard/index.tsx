import React, { memo, useRef } from 'react';
import { useStyles } from './style';
import { IconfontSvg } from '@chat2db/ui';
import { Button } from 'antd';
import { IPriceData } from '@/typings/pricing';
import TeamSeatModal, { TeamSeatModalRef } from '../TeamSeatModal';

export interface ICardProps {
  priceData: IPriceData;
}

export default memo<ICardProps>((props) => {
  const { priceData } = props;
  const { styles, cx } = useStyles({ active: priceData.mostPopular });
  const teamSeatModalRef = useRef<TeamSeatModalRef>(null);
  const [loading, setLoading] = React.useState(false);

  const buyPlan = async () => {
    if (!priceData.id || loading) return;
    setLoading(true);
    try {
      if (priceData.type === 'team') {
        teamSeatModalRef.current?.setSeatModalVisible(true);
      } else {
        await teamSeatModalRef.current?.handleOpenStripePage(priceData);
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <div key={priceData.title} className={cx(styles.cardBox, { [styles.cardBoxActive]: priceData.mostPopular })}>
      <div className={styles.headerContainer}>
        <div className={styles.firstLine}>{priceData.title}</div>
        <div className={styles.secondLine}>{priceData.subtitle}</div>
        <div className={styles.thirdLine}>
          <div className={styles.thirdLine1}>{priceData.curPrice}</div>
          <div className={styles.thirdLine2}>{priceData.priceSuffix}</div>
        </div>
        <div className={styles.thirdLine3}>{priceData.thenPriceSuffix && priceData.thenPriceSuffix}</div>
      </div>
      <Button type="primary" className={styles.payButton} onClick={buyPlan} loading={loading}>
        {priceData.buyButtonText}
      </Button>
      <ul role="list" className={styles.ul}>
        {priceData.features.map((feature) => (
          <li key={feature.label} className={styles.uiLi}>
            <IconfontSvg className={styles.check} size={16} code="icon-danse" />
            {feature.label}
          </li>
        ))}
      </ul>
      <TeamSeatModal priceData={priceData} ref={teamSeatModalRef} />
    </div>
  );
});

import { IOrganizationVO } from '@/typings/enterprise/organization';
import { useStyles } from './style';
import Avatar from '@/components/Avatar';

interface IProps {
  org: IOrganizationVO | null;
}

const OrgItem = (props: IProps) => {
  const { styles, cx } = useStyles();
  const { org } = props;

  if (!org) return null;

  return (
    <div className={cx(styles.item)}>
      <div className={styles.itemLeft}>
        <Avatar className={styles.avatar} org={org} size={24} />
        <div className={styles.itemName}>{org.name}</div>
        <div className={styles.itemTag}>{org.vip ? 'Pro' : 'Free'}</div>
      </div>
    </div>
  );
};

export default OrgItem;

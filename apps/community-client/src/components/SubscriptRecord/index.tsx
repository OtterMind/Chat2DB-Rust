import { useOrgStore } from '@/store/organization';
import { useUserStore } from '@/store/user';
import { Empty, EmptyImage, Modal } from '@chat2db/ui';
import RecordItem from './RecordItem';
import { useStyles } from './style';

const SubscriptRecord = () => {
  const { subscriptModalStatus, setSubscriptModalStatus } = useUserStore((s) => ({
    subscriptModalStatus: s.subscriptModalStatus,
    setSubscriptModalStatus: s.setSubscriptModalStatus,
  }));

  const curOrgSubscription = useOrgStore((s) => s.curOrgSubscription);

  const { styles } = useStyles();

  const renderContent = () => {
    if (!curOrgSubscription?.items || curOrgSubscription?.items.length === 0) {
      return (
        <Empty
          className={styles.empty}
          image={EmptyImage.ChartList}
          title="目前暂无订阅记录"
          subTitle="快去订阅团队套餐，开启团队协作工作新方式"
          buttonText="去订阅套餐"
          onButtonClick={() => {
            console.log('去订阅套餐');
            setSubscriptModalStatus(false);
          }}
        />
      );
    }

    return (curOrgSubscription?.items || []).map((item) => {
      return <RecordItem key={item.id} item={item} />;
    });
  };

  return (
    <Modal
      width={450}
      maxHeight={'80vh'}
      open={subscriptModalStatus}
      title={'订阅记录'}
      headerIconCode={'icon-formatting'}
      footer={null}
      onCancel={() => setSubscriptModalStatus(false)}
      centered
      maskClosable={false}
      className={styles.wrapper}
    >
      {renderContent()}
    </Modal>
  );
};

export default SubscriptRecord;

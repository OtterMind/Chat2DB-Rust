import { memo, useMemo } from 'react';
import i18n from '@/i18n';
import { Input } from 'antd';
import { IconButton } from '@chat2db/ui';
import { useStyles } from './style';
import { RotateCcw } from 'lucide-react';

// ----- components -----
import Iconfont from '@/components/Iconfont';

// ----- store -----
import { useWorkspaceStore } from '@/store/workspace';
import { canCreateDatabase, canCreateSchema } from '@/utils/databaseJudgments';

interface IProps {
  searchValue: string;
  setSearchValue: (value: string) => void;
  getTreeData: (refresh?: boolean) => void;
}

const OperationLine = (props: IProps) => {
  const { searchValue, setSearchValue, getTreeData } = props;
  const { styles } = useStyles();
  const { currentConnectionDetails, openCreateDatabaseModal } = useWorkspaceStore((state) => {
    return {
      currentConnectionDetails: state.currentConnectionDetails,
      openCreateDatabaseModal: state.openCreateDatabaseModal,
    };
  });

  const handelOpenCreateDatabaseModal = () => {
    const type = currentConnectionDetails?.supportDatabase ? 'database' : 'schema';

    openCreateDatabaseModal?.({
      type,
      relyOnParams: {
        databaseType: currentConnectionDetails!.type!,
        dataSourceId: currentConnectionDetails!.id!,
      },
      executedCallback: () => {
        getTreeData(true);
      },
    });
  };

  const showCreate = useMemo(() => {
    if (currentConnectionDetails?.supportDatabase) {
      return canCreateDatabase(currentConnectionDetails!.type!);
    }
    if (currentConnectionDetails?.supportSchema) {
      return canCreateSchema(currentConnectionDetails!.type!);
    }
  }, [currentConnectionDetails]);

  return (
    <>
      <div className={styles.operationLine}>
        <div className={styles.operationLineLeft}>
          {showCreate && <IconButton onClick={handelOpenCreateDatabaseModal} icon="&#xeb78;" size="sm" />}
          <IconButton
            onClick={() => {
              getTreeData(true);
            }}
            icon={RotateCcw}
            size="sm"
          />
        </div>
      </div>
      <div className={styles.searchBox}>
        <Input
          size="small"
          prefix={<Iconfont code="&#xe888;" />}
          value={searchValue}
          onChange={(e) => setSearchValue(e.target.value)}
          allowClear
          placeholder={i18n('workspace.tree.search.placeholder')}
        />
      </div>
    </>
  );
};

export default memo(OperationLine);

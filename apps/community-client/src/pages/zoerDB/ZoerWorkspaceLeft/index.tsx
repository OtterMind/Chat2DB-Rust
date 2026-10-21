import { memo, useEffect, useState } from 'react';
import { useStyles } from './style';
import sqlServices from '@/service/sql';
import { useWorkspaceStore } from '@/store/workspace';
import { OperationColumn, WorkspaceTabType } from '@/constants';
import { IconButton, IconfontSvg } from '@chat2db/ui';
import LoadingContent from '@/components/Loading/LoadingContent';
import i18n from '@/i18n';

interface IProps {
  className?: string;
  boundInfo: {
    dataSourceId: number;
    databaseName?: string;
    schemaName?: string;
  };
}

export default memo<IProps>((props) => {
  const { className, boundInfo } = props;
  const {
    styles,
    cx,
    theme: { appearance },
  } = useStyles();
  const [tableList, setTableList] = useState<any[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [activeTable, setActiveTable] = useState<string | null>(null);

  const { addWorkspaceTab } = useWorkspaceStore((state) => {
    return {
      addWorkspaceTab: state.addWorkspaceTab,
    };
  });

  const getTableList = () => {
    setIsLoading(true);
    sqlServices
      .getTableList({
        ...boundInfo,
        pageNo: 1,
        pageSize: 1000,
        refresh: true,
      })
      .then((res) => {
        setTableList(res.data);
      })
      .finally(() => {
        setIsLoading(false);
      });
  };

  useEffect(() => {
    getTableList();
  }, []);

  const handleClick = (tableItem) => {
    setActiveTable(tableItem.name);
    addWorkspaceTab({
      id: `${OperationColumn.OpenTable}-${tableItem.name}`,
      title: tableItem.name,
      type: WorkspaceTabType.EditTableData,
      uniqueData: {
        ...boundInfo,
        tableName: tableItem.name,
      },
    });
  };

  return (
    <div className={cx(styles.container, className)}>
      <div className={styles.header}>
        <div className={styles.headerTitle}>{i18n('workspace.text.databaseTable')}</div>
        <IconButton size="md" code="icon-refresh" spin={isLoading} onClick={getTableList} />
      </div>
      <LoadingContent className={styles.tableBox} isLoading={isLoading}>
        {tableList.map((item) => {
          return (
            <div
              key={item.tableName}
              onClick={() => {
                handleClick(item);
              }}
              className={cx(styles.tableItem, { [styles.activeTableItem]: activeTable === item.name })}
              tabIndex={0}
            >
              <IconfontSvg code="icon-colourful-table" appearance={appearance} existDark={true} />
              <div className={styles.textContent}>
                <span className={styles.tableName}>{item.name}</span>
                {item.comment && <span className={styles.tableComment}>({item.comment})</span>}
              </div>
            </div>
          );
        })}
      </LoadingContent>
    </div>
  );
});

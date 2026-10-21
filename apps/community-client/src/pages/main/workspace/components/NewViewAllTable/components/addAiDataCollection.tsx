// Pin the table to the top.
import { useEffect, useMemo, useState } from 'react';
import { Button, Select, Space } from 'antd';
import i18n from '@/i18n';
import { openModal } from '@/store/common/components';
import ModalTitle from '@/components/Modal/ModalTitle';
import { createStyles, css } from 'antd-style';
import aiDataCollectionService from '@/service/aiDataCollection';
import { IconfontSvg } from '@chat2db/ui';
import { CollectionSource, DataCollectionElementType } from '@/constants/aiDataCollection';
import { useTreeStore } from '@/store/tree';

export const useStyles = createStyles(({ token }) => {
  return {
    modalContent: css`
      padding-top: 20px;
      .ant-input-affix-wrapper-lg {
        height: 42px;
      }
    `,
    footer: css`
      display: flex;
      justify-content: flex-end;
      margin-top: 24px;
      gap: 8px;
    `,
    syncTips: css`
      display: flex;
      align-items: center;
      gap: 4px;
      color: ${token.colorTextSecondary};
      font-size: 12px;
      margin-top: 8px;
    `,
    popupClassName: css`
      .ant-select-item-empty {
        padding: 0px;
      }
    `,
    createAiDataCollectionButton: css`
      color: ${token.colorText};
      cursor: pointer;
      padding: 5px 12px;
      border-radius: 4px;
      font-weight: bold;
      &:hover {
        background-color: ${token.colorPrimaryBg};
      }
    `,
    createAiDataCollectionTips: css`
      color: ${token.colorTextSecondary};
      padding: 5px 12px;
      border-radius: 4px;
    `,
  };
});

interface IProps {
  tableNameList: string[];
  dataCollectionElementType?: DataCollectionElementType;
  loadData: any;
  parentInfo: any;
  aiDataCollectionId?: number;
}

export const openAddAiDataCollectionModal = ({
  tableNameList,
  dataCollectionElementType,
  loadData,
  parentInfo,
  aiDataCollectionId,
}: IProps) => {
  openModal({
    width: '700px',
    title: (
      <ModalTitle
        title={i18n('workspace.aiDataCollection.select')}
        iconCode="icon-folder-close-ai"
        tips={{
          text: i18n('workspace.aiDataCollection.doc'),
          url: 'https://docs.chat2db-ai.com/docs/ai-chat/ai-data-collection',
        }}
      />
    ),
    content: (
      <AddAiDataCollectionModalContent
        dataCollectionElementType={dataCollectionElementType}
        tableNameList={tableNameList}
        parentInfo={parentInfo}
        loadData={loadData}
        openModal={openModal}
        aiDataCollectionId={aiDataCollectionId}
      />
    ),
  });
};

export const AddAiDataCollectionModalContent = (params: {
  parentInfo: any;
  tableNameList: any;
  openModal: any;
  loadData: any;
  dataCollectionElementType?: DataCollectionElementType;
  aiDataCollectionId?: number;
}) => {
  const { loadData, tableNameList, parentInfo, aiDataCollectionId, dataCollectionElementType } = params;
  const {
    styles,
    theme: { appearance },
  } = useStyles();
  const [selectedKey, setSelectedKey] = useState<number | null | undefined>(aiDataCollectionId);
  const [options, setOptions] = useState<any>([]);
  const [loading, setLoading] = useState(false);
  const [searchValue, setSearchValue] = useState('');
  const [open, setOpen] = useState<false | undefined>(undefined);
  const refreshAiDataCollection = useTreeStore((s) => s.refreshAiDataCollection);

  useEffect(() => {
    getOptions();
  }, []);

  const getOptions = () => {
    aiDataCollectionService
      .getAiDataCollectionList({
        dataSourceId: parentInfo.dataSourceId,
        pageNo: 1,
        pageSize: 1000,
      })
      .then((res) => {
        const _options = res.data.map((item) => {
          return {
            value: item.id,
            label: item.title,
            id: item.id,
          };
        });
        setOptions(_options);
      });
  };

  const onOk = () => {
    if (!selectedKey) {
      return;
    }
    const list = tableNameList.map((item) => {
      return {
        dataSourceId: parentInfo.dataSourceId,
        databaseName: parentInfo.databaseName,
        schemaName: parentInfo.schemaName,
        tableName: item,
      };
    });

    const param = {
      id: selectedKey,
      dataSourceId: parentInfo.dataSourceId,
      databaseName: parentInfo.databaseName,
      schemaName: parentInfo.schemaName,
      type: dataCollectionElementType,
      elements: list,
    };

    setLoading(true);

    aiDataCollectionService
      .addAiDataCollectionElement(param)
      .then(() => {
        openModal(false);
        loadData();
        refreshAiDataCollection(parentInfo.dataSourceId);
      })
      .finally(() => {
        setLoading(false);
      });
  };

  const handleChange = (value: number) => {
    console.log(value);
    setSelectedKey(value);
  };

  const filterOption = (input: string, option?: { label: string; value: string }) =>
    (option?.label ?? '').toLowerCase().includes(input.toLowerCase());

  const createAiDataCollection = () => {
    const param = {
      title: searchValue,
      dataSourceId: parentInfo.dataSourceId,
      collectionSource: CollectionSource.DATA_SOURCE,
    };

    aiDataCollectionService.createAiDataCollection(param).then((res) => {
      getOptions();
      handleChange(res);
      setOpen(false);
      setTimeout(() => {
        setOpen(undefined);
      }, 0);
    });
  };

  const notFoundContent = useMemo(() => {
    return searchValue ? (
      <div className={styles.createAiDataCollectionButton} onClick={createAiDataCollection}>
        {i18n('workspace.aiDataCollection.create')}: {searchValue}
      </div>
    ) : (
      <div className={styles.createAiDataCollectionTips}>{i18n('workspace.aiDataCollection.createTips')}</div>
    );
  }, [searchValue]);

  return (
    <div className={styles.modalContent}>
      <Select
        showSearch
        size="large"
        placeholder={i18n('workspace.tips.searchAiDataCollection')}
        onChange={handleChange}
        searchValue={searchValue}
        onSearch={setSearchValue}
        style={{ width: '100%' }}
        filterOption={filterOption}
        options={options}
        popupClassName={styles.popupClassName}
        notFoundContent={notFoundContent}
        optionRender={(option) => (
          <Space>
            <IconfontSvg existDark appearance={appearance} code="icon-colourful-folder-close-ai" />
            {option.data.label}
          </Space>
        )}
        open={open}
        value={selectedKey}
      />
      {loading && (
        <div className={styles.syncTips}>
          <span>{i18n('workspace.aiDataCollection.syncing')}</span>
        </div>
      )}
      <div className={styles.footer}>
        <Button
          onClick={() => {
            openModal(false);
          }}
        >
          {i18n('common.button.cancel')}
        </Button>
        <Button type="primary" loading={loading} onClick={onOk}>
          {i18n('common.button.affirm')}
        </Button>
      </div>
    </div>
  );
};

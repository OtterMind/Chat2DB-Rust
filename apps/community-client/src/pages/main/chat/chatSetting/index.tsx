import { FC, useEffect, useMemo, useState } from 'react';
import { ChatVO } from '@/typings/chat';
import { ChatSourceType, ChatCornerstoneType } from '@/constants/chat';
import { Modal, ModalProps, Select, IconfontSvg, Iconfont } from '@chat2db/ui';
import { Button, Form, Input } from 'antd';
import { useStyles } from './styles';
import { useForm } from 'antd/es/form/Form';
import aiDataCollectionService from '@/service/aiDataCollection';
import i18n from '@/i18n';
import { useChatStore } from '@/store/chat';
import ModalTitle from '@/components/Modal/ModalTitle';
import UploadLocalFile, { FileUrl } from '@/components/UploadLocalFile';
import { isDevelopment } from '@/utils/env';
import ExcelFiltersTableHeaders from '@/components/ExcelFiltersTableHeaders';

export interface ChatSettingProps extends ModalProps {
  chatItem?: ChatVO;
}

interface CreateChatParams {
  aiDataCollectionId?: number;
  fileUrlList?: FileUrl[];
  type: ChatCornerstoneType;
}

const ChatSetting: FC<ChatSettingProps> = () => {
  const { styles, cx } = useStyles();
  const [form] = useForm();
  const [aiDataCollectionOptions, setAiDataCollectionOptions] = useState<any>([]);
  const { isOpenSetting, setOpenSettingModal, setSettingInfo, createNewChat, settingInfo } = useChatStore((state) => {
    return {
      isOpenSetting: state.isOpenSetting,
      settingInfo: state.settingInfo,
      setOpenSettingModal: state.setOpenSettingModal,
      setSettingInfo: state.setSettingInfo,
      createNewChat: state.createNewChat,
    };
  });
  const [createParams, setCreateParams] = useState<CreateChatParams>({
    type: ChatCornerstoneType.EXCEL,
  });
  const [formValue, setFormValue] = useState<any>({});
  const [isCreating, setIsCreating] = useState(false);
  const [openExcelCheck, setOpenExcelCheck] = useState(false);

  const getAiDataCollectionList = () => {
    aiDataCollectionService.getAiDataCollectionList({ pageNo: 1, pageSize: 1000 }).then((res) => {
      const list = res.data.map((item) => {
        return {
          value: item.id,
          label: item.title,
        };
      });
      setAiDataCollectionOptions(list);
    });
  };

  useEffect(() => {
    if (isOpenSetting) {
      form.setFieldsValue(settingInfo);
      getAiDataCollectionList();
    } else {
      form.resetFields();
      setSettingInfo(undefined);
    }
  }, [isOpenSetting]);

  const handleOk = () => {
    if (createParams.type === ChatCornerstoneType.EXCEL) {
      setOpenExcelCheck(true);
      setOpenSettingModal(false);
      return;
    }
    setIsCreating(true);
    createNewChat({
      dataSourceCollectionId: createParams.aiDataCollectionId,
      filePath: createParams.fileUrlList?.[0]?.filePath,
      fileName: createParams.fileUrlList?.[0]?.fileName,
      source: ChatSourceType.CHAT,
    }).finally(() => {
      setIsCreating(false);
    });
  };

  const onCancel = () => {
    setOpenSettingModal(false);
  };

  const chatCornerstoneTypeConfig = useMemo(() => {
    return [
      {
        icon: (
          <div className={styles.excelIconBox}>
            <IconfontSvg className={styles.icon} code="icon-chat-excel" />
          </div>
        ),
        title: (
          <div className={styles.excelTitleBox}>
            {i18n('chat.cornerstoneType.excel.title')}
            <div className={styles.hotIconBox}>
              <Iconfont code="icon-gradient-hot" gradientColor="linear-gradient(to right, #FFC634 0%, #FF6433 100%)" />
            </div>
          </div>
        ),
        description: i18n('chat.cornerstoneType.excel.description'),
        value: ChatCornerstoneType.EXCEL,
      },
      {
        icon: (
          <div className={styles.databaseIconBox}>
            <IconfontSvg className={styles.icon} code="icon-chat-database" />
          </div>
        ),
        title: i18n('chat.cornerstoneType.database.title'),
        description: i18n('chat.cornerstoneType.database.description'),
        value: ChatCornerstoneType.DATABASE,
      },
    ];
  }, []);

  const excelTips = () => {
    return <div className={styles.excelTips}>✨ {i18n('chat.uploadExcel.uploadExcelTips')}</div>;
  };

  const renderChatCornerstoneType = () => {
    return (
      <div className={styles.chatCornerstoneTypeItemList}>
        {chatCornerstoneTypeConfig.map((item) => {
          return (
            <div
              key={item.value}
              className={cx(styles.chatCornerstoneTypeItem, {
                [styles.chatCornerstoneTypeItemActive]: createParams.type === item.value,
              })}
              onClick={() => {
                setCreateParams({
                  type: item.value,
                });
                form.resetFields();
              }}
            >
              <div className={styles.iconBox}>{item.icon}</div>
              <div className={styles.rightBox}>
                <div className={styles.typeTitle}>{item.title}</div>
                <div className={styles.typeDescription}>{item.description}</div>
              </div>
            </div>
          );
        })}
      </div>
    );
  };

  const handleFileUrlListChange = (fileUrlList: FileUrl[]) => {
    setCreateParams({
      ...createParams,
      fileUrlList,
    });
  };

  const handleFormChange = (changedValues, allValues) => {
    setFormValue({
      ...formValue,
      ...allValues,
    });

    if (changedValues.fileUrl) {
      setCreateParams({
        ...createParams,
        fileUrlList: [
          {
            filePath: changedValues.fileUrl,
            fileName: changedValues.fileUrl,
          },
        ],
      });
    }
  };

  const checkExcelSubmitCallBack = (data: any) => {
    createNewChat({
      filePath: createParams.fileUrlList?.[0]?.filePath,
      fileName: createParams.fileUrlList?.[0]?.fileName,
      excelConfig: data,
      source: ChatSourceType.EXCEL_CHAT,
    }).finally(() => {
      setIsCreating(false);
      setOpenExcelCheck(false);
    });
  };

  const handleCloseCallBack = () => {
    setOpenExcelCheck(false);
  };

  return (
    <>
      <Modal
        title={
          <ModalTitle
            iconCode="icon-AI"
            title={settingInfo?.id ? i18n('chat.edit.title') : i18n('chat.create.title')}
          />
        }
        open={isOpenSetting}
        onCancel={onCancel}
        width={600}
        maskClosable={false}
        footer={
          <>
            <Button
              onClick={() => {
                setOpenSettingModal(false);
              }}
            >
              {i18n('common.button.cancel')}
            </Button>
            <Button
              type="primary"
              loading={isCreating}
              disabled={
                !(
                  (createParams.type === ChatCornerstoneType.EXCEL && createParams.fileUrlList?.length) ||
                  (createParams.type === ChatCornerstoneType.DATABASE && createParams.aiDataCollectionId)
                )
              }
              onClick={handleOk}
            >
              {i18n('common.button.affirm')}
            </Button>
          </>
        }
        destroyOnClose={true}
      >
        {renderChatCornerstoneType()}
        <div className={styles.selectData}>
          <Form
            layout="vertical"
            form={form}
            onValuesChange={handleFormChange}
            initialValues={{ remember: true }}
            autoComplete="off"
          >
            {createParams.type === ChatCornerstoneType.EXCEL && (
              <>
                <Form.Item>
                  <UploadLocalFile
                    fileUrlListChange={handleFileUrlListChange}
                    accept=".xlsx,.xls,.csv"
                    description={[i18n('chat.uploadExcel.description1'), i18n('chat.uploadExcel.description2')]}
                    descriptionSlot={excelTips()}
                  />
                </Form.Item>
                {isDevelopment && (
                  <Form.Item label="File URL(仅仅为了桌面端测试环境方便测试)" name="fileUrl">
                    <Input autoComplete="off" />
                  </Form.Item>
                )}
              </>
            )}
            {createParams.type === ChatCornerstoneType.DATABASE && (
              <Form.Item
                label={i18n('common.text.aiDataCollection')}
                name="aiDataCollectionId"
                rules={[{ required: true, message: i18n('common.form.error.required') }]}
              >
                <Select
                  options={aiDataCollectionOptions}
                  placeholder={i18n('workspace.aiDataCollection.select.placeholder')}
                  style={{ width: '100%' }}
                  onChange={(value) => {
                    setCreateParams({
                      ...createParams,
                      aiDataCollectionId: value,
                    });
                  }}
                />
              </Form.Item>
            )}
          </Form>
        </div>
      </Modal>
      <Modal
        title={<ModalTitle iconCode="icon-chat-excel" title={i18n('chat.excelPreview.title')} />}
        width="90vw"
        height="90vh"
        centered
        open={openExcelCheck}
        footer={null}
        headerBorder
        className={styles.checkExcelModal}
        destroyOnClose
        maskClosable={false}
        onCancel={() => {
          setOpenExcelCheck(false);
        }}
      >
        {createParams?.fileUrlList?.[0] && (
          <ExcelFiltersTableHeaders
            closeCallBack={handleCloseCallBack}
            submitCallBack={checkExcelSubmitCallBack}
            fileUrl={createParams.fileUrlList[0]}
          />
        )}
      </Modal>
    </>
  );
};

export default ChatSetting;

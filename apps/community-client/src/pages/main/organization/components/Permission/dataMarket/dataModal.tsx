import { FC, useEffect } from 'react';
import { CollectionSourceDataType, IDataSourceCollection } from '@/typings/dataMarket';
import { Modal, Input, staticMessage } from '@chat2db/ui';
import { Form } from 'antd';
import dataMarkerService from '@/service/dataMarket';
import { useConnectionStore } from '@/store/connection';
import CascaderDBList from './cascaderDBList';

interface DataMarketModalProps {
  openModal: boolean;
  setOpenModal: (open: boolean) => void;
  dataItem?: IDataSourceCollection;
  onConfirm?: () => void;
}

const DataMarketModal: FC<DataMarketModalProps> = ({ openModal, dataItem, setOpenModal, onConfirm }) => {
  const [form] = Form.useForm();
  const dataSourceList = useConnectionStore((state) => state.connectionList);

  useEffect(() => {
    if (dataItem) {
      form.setFieldsValue(dataItem);
    }
  }, [dataItem]);

  const handleCascaderChange = (cascaderValue: any) => {
    form.setFieldValue('elements', cascaderValue);
  };

  const handleOk = () => {
    const formValues = form.getFieldsValue();
    const { elements, title } = formValues;

    const request = dataItem?.id ? dataMarkerService.updateCollection : dataMarkerService.createCollection;

    request({
      ...dataItem,
      title,
      elements,
      collectionSource: CollectionSourceDataType.DATA_SOURCE,
    }).then(() => {
      staticMessage.success(dataItem?.id ? '更新成功' : '创建成功');
      onConfirm && onConfirm();
      handleCancel();
    });
  };

  const handleCancel = () => {
    setOpenModal(false);
    form.resetFields();
  };

  return (
    <Modal
      open={openModal}
      title={dataItem?.id ? '编辑数据集' : '新增数据集'}
      onOk={handleOk}
      onCancel={handleCancel}
      destroyOnClose
      maskClosable={false}
    >
      <Form form={form} layout="vertical">
        <Form.Item name={'title'} label="标题">
          <Input />
        </Form.Item>
        <Form.Item name="elements" label="数据源集合">
          <CascaderDBList
            defaultValue={dataItem?.elements}
            onChange={handleCascaderChange}
            dataSourceList={dataSourceList}
          />
        </Form.Item>
      </Form>
    </Modal>
  );
};

export default DataMarketModal;

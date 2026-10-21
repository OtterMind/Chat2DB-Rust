import { useEffect, useRef, useState } from 'react';
import { IApplyRunScript } from '@/typings/enterprise/permission';
import { Button, DatePicker, Drawer, Form, Input, Radio } from 'antd';
import dayjs from 'dayjs';
import CascaderDB from '@/components/CascaderDB';
import { BooleanType } from '@/typings/common';
import ApprovalFlow from '@/pages/main/organization/components/Approval/approvalFlow';
import SearchResult, { ISearchResultRef } from '@/blocks/SearchResult';
import { ApprovalStatusType } from '@/typings/enterprise/approval';
import styles from './index.less';

export type IScriptForm = {
  id?: number;
  name: string;
  description: string;
  noExpire: BooleanType;
  validUntil?: number;
  databaseInfo?: {
    dataSourceId: number;
    databaseName: string;
    schemaName: string;
  };
  scriptContent: string;
  approvalId?: number;
  approvalStatus?: ApprovalStatusType;
};
// Partial<
//   IApplyRunScript & { databaseInfo: any; approvalId: number; approvalStatus: string }
// >;
interface IProps {
  isPreview?: boolean;
  initData: IScriptForm;
  onFormChange?: (allValues: IApplyRunScript) => void;
}

function ScriptForm(props: IProps) {
  const { isPreview, initData } = props;

  const [openWorkflow, setOpenWorkflow] = useState(false);
  const [hasClickRunScript, setHasClickRunScript] = useState(false); // Whether Run Script has been clicked.
  const searchResultRef = useRef<ISearchResultRef>(null);
  const [form] = Form.useForm();
  const isNoExpire = Form.useWatch('noExpire', form);

  useEffect(() => {
    if (isPreview) {
      form.setFieldsValue({
        ...initData,
        validUntil: props.initData?.validUntil ? dayjs(props.initData?.validUntil) : undefined,
      });
    } else {
      form.setFieldsValue({
        noExpire: BooleanType.No,
      });
    }
  }, []);

  const handleFormChange = (changedValues: any, allValues: IApplyRunScript) => {
    /** onChange is needed only in edit mode. */
    if (isPreview) return;

    const { validUntil } = allValues;
    props.onFormChange &&
      props.onFormChange({ ...allValues, validUntil: validUntil ? dayjs(validUntil).valueOf() : undefined });
  };

  const renderApprovalFlow = () => {
    const { id, approvalId, approvalStatus, scriptContent, databaseInfo } = initData || {};
    const { dataSourceId, databaseName } = databaseInfo || {};

    if (!approvalId) return null;
    return (
      <div className={styles.workflow}>
        <Button type="primary" onClick={() => setOpenWorkflow(true)}>
          查看审批流
        </Button>

        {approvalStatus === 'APPROVED' && scriptContent ? (
          <>
            <Button
              style={{ marginLeft: '32px' }}
              onClick={() => {
                setHasClickRunScript(true);
                setTimeout(() => {
                  searchResultRef.current?.handleExecuteSQL({ sql: scriptContent, applyId: id });
                }, 100);
              }}
            >
              运行脚本
            </Button>
            {hasClickRunScript && (
              <div style={{ marginTop: '32px' }}>
                <SearchResult
                  ref={searchResultRef}
                  executeSqlParams={{
                    dataSourceId,
                    databaseName,
                  }}
                />
              </div>
            )}
          </>
        ) : null}

        <Drawer title={'查看审批流'} open={openWorkflow} width={720} onClose={() => setOpenWorkflow(false)}>
          <ApprovalFlow approvalId={approvalId} />
        </Drawer>
      </div>
    );
  };

  return (
    <>
      <Form
        layout="vertical"
        labelCol={{ span: 4 }}
        className={styles.formWrapper}
        form={form}
        onValuesChange={handleFormChange}
        autoComplete="off"
        disabled={isPreview}
      >
        <Form.Item
          label="名称"
          name="name"
          rules={[
            {
              required: true,
              message: '必填',
            },
          ]}
        >
          <Input maxLength={50} />
        </Form.Item>

        <Form.Item
          label="描述"
          name="description"
          rules={[
            {
              required: true,
              message: '必填',
            },
          ]}
        >
          <Input.TextArea maxLength={250} />
        </Form.Item>

        <Form.Item label="是否永久有效" name="noExpire">
          <Radio.Group disabled>
            <Radio value={BooleanType.Yes}>是</Radio>
            <Radio value={BooleanType.No}>否</Radio>
          </Radio.Group>
        </Form.Item>

        {isNoExpire === BooleanType.Yes ? null : (
          <Form.Item
            label="权限到期时间"
            name="validUntil"
            rules={[
              {
                required: true,
                message: '必填',
              },
            ]}
          >
            <DatePicker showTime disabledDate={(current) => current < dayjs().startOf('day')} />
          </Form.Item>
        )}

        <Form.Item
          label="数据库信息"
          name="databaseInfo"
          rules={[
            {
              required: true,
              message: '必填',
            },
          ]}
        >
          <CascaderDB
            initData={initData?.databaseInfo}
            disabled={isPreview ? ['datasource', 'database', 'schema'] : []}
            notShowList={['table', 'column']}
          />
        </Form.Item>

        <Form.Item
          label="脚本内容"
          name="scriptContent"
          rules={[
            {
              required: true,
              message: '必填',
            },
          ]}
        >
          <Input.TextArea />
        </Form.Item>
      </Form>

      {renderApprovalFlow()}
    </>
  );
}

export default ScriptForm;

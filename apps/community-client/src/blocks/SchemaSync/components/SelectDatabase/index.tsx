import { memo, useEffect, useState, forwardRef, useImperativeHandle, ForwardedRef } from 'react';
import { useStyles } from './style';
import { Form, Select } from 'antd';
import i18n from '@/i18n';
import { IconfontSvg } from '@chat2db/ui';
import { databaseMap } from '@/constants/database';
import { DatabaseTypeCode } from '@/constants';
import useSelectDatabase, { type ISelectDatabase } from '@/hooks/useSelectDatabase';
import { useUpdateEffect } from 'ahooks';

export type IDiffData = { source: ISelectDatabase; target: ISelectDatabase } | null;

interface IProps {
  className?: string;
  onChanges?: (values: IDiffData) => void;
  initSourceData?: ISelectDatabase;
}

export default memo<IProps>((props) => {
  const { className, onChanges, initSourceData } = props;
  const { styles, cx } = useStyles();
  const [sourceData, setSourceData] = useState<ISelectDatabase>();
  const [targetData, setTargetData] = useState<ISelectDatabase>();

  useEffect(() => {
    if (sourceData && targetData && sourceData.selectDone && targetData.selectDone) {
      onChanges?.({
        source: {
          dataSourceId: sourceData?.dataSourceId,
          databaseName: sourceData?.databaseName,
          schemaName: sourceData?.schemaName,
        },
        target: {
          dataSourceId: targetData?.dataSourceId,
          databaseName: targetData?.databaseName,
          schemaName: targetData?.schemaName,
        },
      });
    } else {
      onChanges?.(null);
    }
  }, [sourceData, targetData]);

  const handleSourceChange = (values: ISelectDatabase | undefined) => {
    setSourceData(values);
  };

  const handleTargetChange = (values: ISelectDatabase | undefined) => {
    setTargetData(values);
  };

  return (
    <div className={cx(styles.container, className)}>
      <div className={styles.selectDatabase}>
        <div className={styles.leftBox}>
          <div className={styles.leftFrom}>
            <div className={styles.fromTitle}>{i18n('workspace.sourceDatabase')}</div>
            <div>
              <SelectDatabaseForm initData={initSourceData} onChanges={handleSourceChange} />
            </div>
            {/* <div className={styles.databaseMessage}>1</div> */}
          </div>
        </div>
        <div className={styles.syncIcon}>
          <IconfontSvg size={40} code="icon-sync-structure" />
        </div>
        <div className={styles.rightBox}>
          <div className={styles.rightFrom}>
            <div className={styles.fromTitle}>{i18n('workspace.targetDatabase')}</div>
            <div>
              <SelectDatabaseForm onChanges={handleTargetChange} />
            </div>
            {/* <div className={styles.databaseMessage}>2</div> */}
          </div>
        </div>
      </div>
    </div>
  );
});

interface ISelectDatabaseFormProps {
  initData?: ISelectDatabase;
  astrictDatabaseType?: DatabaseTypeCode; // Limits the database types allowed to be selected
  onChanges?: (values: ISelectDatabase | undefined) => void;
}

interface ISelectDatabaseFormRef {
  form: any;
}

const SelectDatabaseForm = memo(
  forwardRef((props: ISelectDatabaseFormProps, ref: ForwardedRef<ISelectDatabaseFormRef>) => {
    const { astrictDatabaseType, initData, onChanges } = props;
    const { styles } = useStyles();
    const [form] = Form.useForm();

    const { dataSourceList, databaseList, schemaList, selectDatabase, onChangeSelectDatabase } = useSelectDatabase({
      astrictDatabaseType,
    });

    useEffect(() => {
      // If there is no dataSourceList onChangeSelectDatabase will be returned
      if (!initData || !dataSourceList?.length) return;
      onChangeSelectDatabase(initData as any);
    }, [dataSourceList]);

    useUpdateEffect(() => {
      form.setFieldsValue(selectDatabase);
      onChanges?.(selectDatabase as any);
    }, [selectDatabase]);

    const dataSourceLabelRender = (option: any) => {
      const intactOption = dataSourceList?.find((item) => item.value === option.value);
      const databaseType = intactOption?.databaseType;
      if (!intactOption || !databaseType || !databaseMap[databaseType]?.icon) {
        return <div>{option.label}</div>;
      }

      const { icon, iconExistDark } = databaseMap[databaseType];

      return (
        <div className={styles.dataSourceLabel}>
          <IconfontSvg existDark={iconExistDark} code={icon} />
          <div>{intactOption.label}</div>
        </div>
      );
    };

    useImperativeHandle(ref, () => ({
      form,
    }));

    return (
      <Form form={form} layout="vertical" autoComplete={'off'} onValuesChange={onChangeSelectDatabase}>
        <Form.Item name="dataSourceId" label={i18n('common.dataSource.title')} className={styles.formItem}>
          <Select
            showSearch
            options={dataSourceList || []}
            labelRender={dataSourceLabelRender}
            optionRender={dataSourceLabelRender}
          />
        </Form.Item>
        {selectDatabase?.supportDatabase !== false && (
          <Form.Item name="databaseName" label={i18n('common.database.title')} className={styles.formItem}>
            <Select showSearch options={databaseList || []} />
          </Form.Item>
        )}
        {selectDatabase?.supportSchema !== false && (
          <Form.Item name="schemaName" label={i18n('common.schema.title')} className={styles.formItem}>
            <Select showSearch options={schemaList || []} />
          </Form.Item>
        )}
      </Form>
    );
  }),
);

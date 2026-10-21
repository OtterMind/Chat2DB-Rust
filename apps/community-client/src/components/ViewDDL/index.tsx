import React, { memo, useEffect } from 'react';
import { useStyles } from './style';
import classnames from 'classnames';
import sqlServer from '@/service/sql';
import SQLPreview from '@/components/SQLPreview';
import { TreeNodeType } from '@/constants';
interface IProps {
  className?: string;
  data: any;
}

export default memo<IProps>((props) => {
  const { className, data } = props;
  const { styles } = useStyles();

  const [sql, setSql] = React.useState('');
  const requestIdRef = React.useRef(0);

  useEffect(() => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;

    if (!data) {
      setSql('');
      return;
    }

    setSql('');

    getDDL(data).then((res) => {
      if (requestIdRef.current !== requestId) {
        return;
      }
      setSql(res || '');
    })
.catch(() => {
      // Keep current error handling behavior in the service layer.
    });
  }, [data]);

  return (
    <div className={classnames(styles.viewDDL, className)}>
      <SQLPreview sql={sql} source="view-ddl" foldable />
    </div>
  );
});

const getDDL = async (data: any) => {
  if (data.treeNodeType === TreeNodeType.VIEW) {
    if (!data.viewName) {
      return '';
    }
    const res = await sqlServer.getViewDetail({
      dataSourceId: data.dataSourceId,
      databaseName: data.databaseName,
      schemaName: data.schemaName,
      tableName: data.viewName,
    });
    return res?.ddl;
  }

  if (data.treeNodeType === TreeNodeType.FUNCTION) {
    if (!data.functionName) {
      return '';
    }
    const res = await sqlServer.getFunctionDetail({
      dataSourceId: data.dataSourceId,
      databaseName: data.databaseName,
      schemaName: data.schemaName,
      functionName: data.functionName,
    });
    return res?.functionBody;
  }

  if (data.treeNodeType === TreeNodeType.PROCEDURE) {
    if (!data.procedureName) {
      return '';
    }
    const res = await sqlServer.getProcedureDetail({
      dataSourceId: data.dataSourceId,
      databaseName: data.databaseName,
      schemaName: data.schemaName,
      procedureName: data.procedureName,
    });
    return res?.procedureBody;
  }

  if (!data.tableName) {
    return '';
  }
  return sqlServer.exportCreateTableSql({
    ...data,
  } as any);
};

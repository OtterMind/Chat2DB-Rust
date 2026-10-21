import { memo } from 'react';
import { IConnectionDetails } from '@/typings';
import ConnectionEdit from '@/components/ConnectionEdit';
import { useStyles } from './style';

// IConnectionDetails All information represents modification
// null Display the list of additions
//  { type: string } Only the database type represents new addition
type IEditConnectionDetail = IConnectionDetails | null | Pick<IConnectionDetails, 'type'>;

interface IProps {
  className?: string;
  // Called with the edited connection data when the user saves.
  onSubmit?: (data: IConnectionDetails) => Promise<any>;
  connectionDetail: IEditConnectionDetail | null | undefined;
  noPermission?: boolean;
  closeCreateConnection: () => void;
}

export default memo<IProps>((props) => {
  const { styles, cx } = useStyles();
  const { className, onSubmit, connectionDetail, closeCreateConnection } = props;

  return (
    <div className={cx(styles.connectionBlock, className)}>
      {connectionDetail && (
        <div className={cx(styles.createConnections, connectionDetail && [styles.showCreateConnections])}>
          <ConnectionEdit
            closeCreateConnection={closeCreateConnection}
            connectionData={connectionDetail as any}
            submit={onSubmit}
          />
        </div>
      )}
    </div>
  );
});

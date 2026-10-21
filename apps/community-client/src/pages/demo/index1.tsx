import { useEffect } from 'react';
import EditTableView from '@/blocks/EditTableView';
import viewServices from '@/service/database/view';

export default function Demo2() {
  useEffect(() => {
    viewServices
      .getViewMeta({
        dataSourceId: 357,
        databaseName: 'er_modal',
        viewName: 'OrderDetailsView',
      })
      .then((res) => {
        console.log(res);
      });
  }, []);

  return <EditTableView />;
}

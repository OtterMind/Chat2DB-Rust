// is responsible for the data request logic of each chart, including automatic polling, manual refresh, etc.
import { useRef,useCallback, useEffect, useState } from 'react';
import { AUTO_REFRESH } from '@/blocks/BI/Chart/constants';
import { getChartByIdAutoExecuteSQL } from '@/service/dashboard';
import { IChartItem } from '@/typings/dashboard';
import { chartDetailNormalization } from '../utils/dataTreating';
import schedule from 'node-schedule';

export interface UseRequestDataProps {
  // chart id
  chartId?: number;
  // refreshRule
  refreshRule?: RefreshRule;
}

// hooks
export interface UseRequestDataReturn {
  chartDetail?: IChartItem;
  // manual refresh
  refreshData?: () => void;
}

export interface RefreshRule { 
  // refresh rules
  refreshType?: AUTO_REFRESH;
  // specific time
  refreshCycle?: string;
}

export const useFetchData = (props: UseRequestDataProps): UseRequestDataReturn => {
  const { chartId, refreshRule } = props;
  const { refreshType, refreshCycle } = refreshRule || {};
  const [chartDetail, setChartDetail] = useState<IChartItem>();
  // Is the scheduled task enabled?
  const enablingScheduledTaskRef = useRef<() => void>();

  const getData = () => { 
    getChartByIdAutoExecuteSQL({
      chartId: chartId!,
      refresh: true,
    }).then((res) => {
      setChartDetail(chartDetailNormalization(res));

      openTask();
    });
  }

  // calls scheduled tasks
  const openTask = () => { 
    if (!enablingScheduledTaskRef.current) {
      enablingScheduledTaskRef.current = enablingScheduledTask({
        refreshType,
        refreshCycle,
      });
    }
  }

  // Start scheduled tasks
  const enablingScheduledTask = (params: RefreshRule) => { 
    const { refreshType: taskRefreshType, refreshCycle: taskRefreshCycle } = params;
    if (taskRefreshType === AUTO_REFRESH.MINUTES && taskRefreshCycle) {
      const rule = new schedule.RecurrenceRule();
      // analysis refresh period is minutes
      const cycleInMinutes = parseInt(taskRefreshCycle, 10);
      rule.minute = new schedule.Range(0, 59, cycleInMinutes);

      const job = schedule.scheduleJob(rule, () => {
        getData();
      });

      // Clean up scheduled tasks
      return () => {
        job.cancel();
      };
    }
  }

  const handleVisibilityChange = useCallback(() => {
    if (document.hidden) {
      enablingScheduledTaskRef.current?.(); // Pause task
      enablingScheduledTaskRef.current = undefined;
    } else {
      openTask();
    }
   }, []);

  useEffect(() => {
    if (!chartId) {
      return;
    }
    enablingScheduledTaskRef.current = undefined;
    getData();

    document.removeEventListener('visibilitychange', handleVisibilityChange);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => { 
      enablingScheduledTaskRef.current?.();
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    }
  }, [chartId]);

  const refreshData = () => { 
    getData();
  }

  return {
    chartDetail,
    refreshData
  };
};

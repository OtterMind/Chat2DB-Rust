import { IChartItem, IDashboardItem, IPageParams, IPageResponse } from '@/typings';
import createRequest from './base';

/** Get report list */
const getDashboardList = createRequest<IPageParams, IPageResponse<IDashboardItem>>('/api/dashboard/list', {
  method: 'get',
});
const getDashboardById = createRequest<{ id: number }, IDashboardItem>('/api/dashboard', { method: 'get' });
/** Create reports */
const createDashboard = createRequest<IDashboardItem, number>('/api/dashboard/create', { method: 'post' });
/** Update report */
const updateDashboard = createRequest<IDashboardItem, boolean>('/api/dashboard/update', { method: 'post' });
/** Delete report */
const deleteDashboard = createRequest<{ id: number }, string>('/api/dashboard', { method: 'delete' });

/** Query chart details based on id */
const getChartById = createRequest<{ id: number }, IChartItem>('/api/v1/chart', { method: 'get' });
/** Query chart details and let the backend execute the associated SQL automatically. */
const getChartByIdAutoExecuteSQL = createRequest<
  {
    chartId: number;
    refresh: boolean;
  },
  IChartItem
>('/api/chart/detail', { method: 'get' });
/** Create charts */
const createChart = createRequest<IChartItem, number>('/api/v1/chart/create', { method: 'post' });
/** Update chart */
const updateChart = createRequest<IChartItem, void>('/api/v1/chart/update', { method: 'post' });
/** Delete chart */
const deleteChart = createRequest<{ id: number }, string>('/api/chart', { method: 'delete' });

export {
  getDashboardList,
  getDashboardById,
  createDashboard,
  updateDashboard,
  deleteDashboard,
  getChartById,
  getChartByIdAutoExecuteSQL,
  createChart,
  updateChart,
  deleteChart,
};

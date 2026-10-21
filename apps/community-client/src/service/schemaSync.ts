import createRequest from './base';

// Get data synchronization sql
const getSchemaSyncSql = createRequest<any, string>('/api/diff/sql', { method: 'post' });

export default {
  getSchemaSyncSql,
};

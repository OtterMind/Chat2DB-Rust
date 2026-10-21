import createRequest from './base';
import { KnowledgeManagementPromptType } from '@/constants/knowledgeManagement';
import { IPageParams, IPageResponse } from '@/typings/common';

/** Get the knowledge management list */
const getList = createRequest<IPageParams & { promptType: KnowledgeManagementPromptType }, IPageResponse<any>>(
  '/api/ai/prompt/rag/list',
  { method: 'get' },
);

/** Added knowledge management */
const save = createRequest<any, any>('/api/ai/prompt/rag/save', { method: 'post' });

/** Update knowledge management */
const update = createRequest<any, any>('/api/ai/prompt/rag/update', { method: 'post' });

/** Delete knowledge management */
const remove = createRequest<any, any>('/api/ai/prompt/rag/delete', { method: 'post' });

const batchRemove = createRequest<any, any>('/api/ai/prompt/rag/delete_batch', { method: 'post' });

/** Export */
const batchExport = createRequest<any, any>('/api/ai/prompt/rag/export_excel', { method: 'post' });

/** Batch import */
const batchImportKnowledgeTerm = createRequest<any, any>('/api/ai/prompt/rag/knowledge_term/import_excel', {
  method: 'post',
  contentType: 'formData',
});
const batchImportBusinessLogic = createRequest<any, any>('/api/ai/prompt/rag/business_logic/import_excel', {
  method: 'post',
  contentType: 'formData',
});
const batchImportSqlTemplate = createRequest<any, any>('/api/ai/prompt/rag/sql_template/import_excel', {
  method: 'post',
  contentType: 'formData',
});

/** Download template */
const downloadTemplate = createRequest<any, any>('/api/ai/prompt/rag/download_excel', { method: 'post' });

export default {
  getList,
  save,
  update,
  remove,
  batchRemove,
  batchExport,
  batchImportKnowledgeTerm,
  batchImportBusinessLogic,
  batchImportSqlTemplate,
  downloadTemplate,
};

import createRequest from './base';
import { IDatabaseBaseInfo, IPageParams, IPageResponse } from '@/typings';
import { AITableComment } from '@/typings/workspace';
import { AiDataCollectionItem } from '@/typings/aiDataCollection';
import { CollectionSource, DataCollectionElementType } from '@/constants/aiDataCollection';

interface GetAiDataCollectionListParams extends IPageParams {
  dataSourceId?: number;
}

interface CreateAiDataCollectionParams {
  title: string;
  dataSourceId: number;
  collectionSource: CollectionSource;
}

interface AiDataCollectionElementRequest extends IDatabaseBaseInfo {
  id?: number;
  tableName: string;
  type?: DataCollectionElementType;
}

interface UpdateAiDataCollectionParams extends IDatabaseBaseInfo {
  id: number;
  elements?: AiDataCollectionElementRequest[];
}

interface AddAiDataCollectionTablesParams extends IDatabaseBaseInfo {
  id: number;
  type: DataCollectionElementType;
  elements: AiDataCollectionElementRequest[];
}

interface UpdateAiDataCollectionTitleParams {
  id: number;
  title?: string;
}

interface GetAiDataCollectionElementListParams extends IPageParams {
  id: number;
}

interface DeleteAiDataCollectionElementParams extends IDatabaseBaseInfo {
  id: number;
  elements: AiDataCollectionElementRequest[];
}

const getAiDataCollectionList = createRequest<GetAiDataCollectionListParams, IPageResponse<AiDataCollectionItem>>(
  '/api/ai/data/collection/list',
  {},
);
const getAiDataCollectionElementList = createRequest<GetAiDataCollectionElementListParams, AiDataCollectionItem>(
  '/api/ai/data/collection/get',
  {},
);
const createAiDataCollection = createRequest<CreateAiDataCollectionParams, number>('/api/ai/data/collection/create', {
  method: 'post',
});
const updateAiDataCollection = createRequest<UpdateAiDataCollectionParams, boolean>(
  '/api/ai/embedding/data/collection/update',
  {
    method: 'post',
  },
);
const updateAiDataCollectionTitle = createRequest<UpdateAiDataCollectionTitleParams, boolean>(
  '/api/v1/ai/embedding/data/collection/update/brief',
  {
    method: 'post',
  },
);
const deleteAiDataCollection = createRequest<{ id: number }, boolean>('/api/v1/ai/embedding/data/collection/delete', {
  method: 'post',
});
const deleteAiDataCollectionElement = createRequest<DeleteAiDataCollectionElementParams, boolean>(
  '/api/v1/ai/embedding/data/collection/removeTable',
  {
    method: 'post',
  },
);
const addAiDataCollectionElement = createRequest<AddAiDataCollectionTablesParams, boolean>(
  '/api/v1/ai/embedding/data/collection/addTable',
  {
    method: 'post',
  },
);
const syncDataCollection = createRequest<{ id: number; dataSourceId: number }, boolean>(
  '/api/v1/ai/embedding/data/collection/sync',
  {
    method: 'post',
  },
);
const getTableComment = createRequest<
  { id: number } | { dataSourceId: number; databaseName: string; schemaName: string; tableName: string; type: string },
  AITableComment
>('/api/v1/ai/embedding/table/comment/get', {});

const saveTableComment = createRequest<AITableComment, boolean>('/api/v1/ai/embedding/table/comment/save', {
  method: 'post',
});

// Delete embedding
const embeddingErase = createRequest<
  IDatabaseBaseInfo & {
    type: DataCollectionElementType;
    names: string[];
  },
  boolean
>('/api/v1/ai/embedding/erase', { method: 'post' });

export default {
  getAiDataCollectionList,
  getAiDataCollectionElementList,
  createAiDataCollection,
  updateAiDataCollection,
  updateAiDataCollectionTitle,
  deleteAiDataCollection,
  deleteAiDataCollectionElement,
  addAiDataCollectionElement,
  syncDataCollection,
  getTableComment,
  saveTableComment,
  embeddingErase,
};

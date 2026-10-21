import createRequest from './base';
import { isDesktop } from '@/utils/env';

export interface IChatAttachment {
  fileName: string;
  fileType: string;
  contentCategory: 'DOCUMENT' | 'TABULAR';
  content: string;
  contentLength?: number;
  truncated?: boolean;
}

const parseUploadedAttachment = createRequest<{ file: File }, IChatAttachment>(
  '/api/v3/ai/chat/attachment/parse/upload',
  {
    method: 'post',
    contentType: 'formData',
  },
);

const parseLocalAttachment = createRequest<{ filePath: string; fileName?: string }, IChatAttachment>(
  '/api/v3/ai/chat/attachment/parse/local',
  {
    method: 'post',
  },
);

async function parseAttachment(input: { file?: File; filePath?: string; fileName?: string }) {
  if (isDesktop) {
    if (!input.filePath) {
      throw new Error('Missing local file path');
    }
    return parseLocalAttachment({
      filePath: input.filePath,
      fileName: input.fileName,
    });
  }

  if (!input.file) {
    throw new Error('Missing file');
  }

  return parseUploadedAttachment({
    file: input.file,
  });
}

export default {
  parseAttachment,
};

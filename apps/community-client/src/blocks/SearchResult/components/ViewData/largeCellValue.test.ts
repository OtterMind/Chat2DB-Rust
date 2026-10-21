import {
  canSubmitLargeCellEdit,
  decodeLargeCellChunk,
  LARGE_CELL_REQUEST_FORMAT,
  LARGE_CELL_VIEWER_MODE,
  LARGE_VALUE_TYPE,
  getLargeCellEditorLimit,
  getLargeCellImagePreviewBlob,
  getLargeCellLoadedBytes,
  getLargeCellTransferFormat,
  getLargeCellViewerValue,
  getNextLargeCellChunkLimit,
} from './largeCellValue';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

assertEqual(
  getLargeCellTransferFormat(),
  LARGE_CELL_REQUEST_FORMAT.BASE64,
  'all viewers share one lossless transfer format',
);

assertEqual(
  getLargeCellLoadedBytes([
    { value: 'abc', offset: 0, nextOffset: 3, eof: false },
    { value: 'def', offset: 3, nextOffset: 6, eof: true },
  ]),
  6,
  'loaded byte count follows chunk offsets',
);

assertEqual(getLargeCellEditorLimit(LARGE_VALUE_TYPE.TEXT), 10 * 1024 * 1024, 'text editor limit is 10 MB');
assertEqual(getLargeCellEditorLimit(LARGE_VALUE_TYPE.BINARY), 50 * 1024 * 1024, 'binary editor limit is 50 MB');

assertEqual(
  getNextLargeCellChunkLimit({
    loadedSize: 50 * 1024 * 1024 - 2,
    editorLimit: 50 * 1024 * 1024,
  }),
  0,
  'base64 load-all stops when the remaining byte budget cannot form a 3-byte group',
);

assertEqual(
  getNextLargeCellChunkLimit({ loadedSize: 0, editorLimit: 10 }),
  9,
  'base64 chunk limits align to a 3-byte boundary',
);

assertEqual(
  canSubmitLargeCellEdit({
    isLargeValue: true,
    viewerMode: LARGE_CELL_VIEWER_MODE.TEXT,
    displayMode: LARGE_VALUE_TYPE.TEXT,
    eof: false,
    loadedSize: 1024,
    editorLimit: 10 * 1024,
  }),
  false,
  'partial text large values cannot be submitted',
);

assertEqual(
  canSubmitLargeCellEdit({
    isLargeValue: true,
    viewerMode: LARGE_CELL_VIEWER_MODE.TEXT,
    displayMode: LARGE_VALUE_TYPE.BINARY,
    eof: true,
    loadedSize: 1024,
    editorLimit: 10 * 1024,
  }),
  false,
  'binary large values cannot be submitted through Monaco',
);

assertEqual(
  canSubmitLargeCellEdit({
    isLargeValue: false,
    viewerMode: LARGE_CELL_VIEWER_MODE.TEXT,
    displayMode: LARGE_VALUE_TYPE.TEXT,
    eof: false,
    loadedSize: 0,
    editorLimit: 10 * 1024,
  }),
  true,
  'normal small cells keep existing edit behavior',
);

const imageBlob = getLargeCellImagePreviewBlob({
    viewerMode: LARGE_CELL_VIEWER_MODE.IMAGE,
    eof: true,
    contentType: 'image/png',
    chunks: [
      decodeLargeCellChunk({
        value: 'aW1hZ2U=',
        offset: 0,
        nextOffset: 5,
        eof: true,
        encoding: LARGE_CELL_REQUEST_FORMAT.BASE64,
      }),
    ],
  });
assertEqual(imageBlob?.size, 5, 'complete image chunks produce a five-byte blob');
assertEqual(imageBlob?.type, 'image/png', 'image blobs preserve the detected content type');

assertEqual(
  getLargeCellImagePreviewBlob({
    viewerMode: LARGE_CELL_VIEWER_MODE.IMAGE,
    eof: false,
    chunks: [
      decodeLargeCellChunk({
        value: 'aW1h',
        offset: 0,
        nextOffset: 3,
        eof: false,
        encoding: LARGE_CELL_REQUEST_FORMAT.BASE64,
      }),
    ],
  }),
  null,
  'partial image chunks do not produce a preview blob',
);

assertEqual(
  getLargeCellViewerValue({
    viewerMode: LARGE_CELL_VIEWER_MODE.IMAGE,
    chunks: [
      decodeLargeCellChunk({
        value: 'aW1hZ2U=',
        offset: 0,
        nextOffset: 5,
        eof: true,
        encoding: LARGE_CELL_REQUEST_FORMAT.BASE64,
      }),
    ],
  }),
  '',
  'image mode does not create an unused text representation',
);

assertEqual(
  getLargeCellViewerValue({
    viewerMode: LARGE_CELL_VIEWER_MODE.TEXT,
    chunks: [
      decodeLargeCellChunk({ value: 'aA==', offset: 0, nextOffset: 1, eof: false, encoding: 'base64' }),
      decodeLargeCellChunk({ value: 'aQ==', offset: 1, nextOffset: 2, eof: true, encoding: 'base64' }),
    ],
  }),
  'hi',
  'text viewer decodes binary base64 chunks as UTF-8',
);

assertEqual(
  getLargeCellViewerValue({
    viewerMode: LARGE_CELL_VIEWER_MODE.TEXT,
    chunks: [
      decodeLargeCellChunk({
        value: '5L2g5aW9',
        offset: 0,
        nextOffset: 6,
        eof: true,
        encoding: LARGE_CELL_REQUEST_FORMAT.BASE64,
      }),
    ],
  }),
  '\u4f60\u597d',
  'text viewer decodes text chunks from the shared base64 transfer',
);

assertEqual(
  getLargeCellViewerValue({
    viewerMode: LARGE_CELL_VIEWER_MODE.HEX,
    chunks: [
      decodeLargeCellChunk({
        value: 'SGk=',
        offset: 0,
        nextOffset: 2,
        eof: true,
        encoding: LARGE_CELL_REQUEST_FORMAT.BASE64,
      }),
    ],
  }),
  '4869',
  'hex viewer converts already loaded bytes locally',
);

console.log('large cell viewer helper tests passed');

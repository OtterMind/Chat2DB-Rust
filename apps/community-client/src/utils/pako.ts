import pako from 'pako';

/** Compressed text string */
export function zipText(_str: unknown): Promise<string> {
  return new Promise((resolve, reject) => {
    let value = '';
    try {
      let str: string;
      if (typeof _str === 'object' && _str !== null) {
        str = JSON.stringify(_str);
      } else if (typeof _str === 'string') {
        str = _str;
      } else {
        throw new Error('Invalid input type');
      }

      value = Buffer.from(pako.gzip(str), 'utf-8').toString('base64');
      resolve(value);
    } catch (e) {
      console.error('GZIP: Failed to compress the text string', e);
      reject(e);
    }
  });
}

/** Decompress text string */
export function unzipText(str: string): Promise<any> {
  return new Promise((resolve, reject) => {
    let value: any = '';
    try {
      const data = pako.ungzip(Buffer.from(str, 'base64'), { to: 'string' });
      try {
        value = JSON.parse(data);
      } catch {
        value = data;
      }
      resolve(value);
    } catch (e) {
      console.error('GZIP: Failed to decompress the text string', e);
      reject(e);
    }
  });
}

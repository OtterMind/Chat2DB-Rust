import { z } from 'zod';
import { DataSourceTreeSchema } from './validation';

// Types defined using zod
export type DataSourceTree = z.infer<typeof DataSourceTreeSchema>;

export interface TreeNodeShowTreeNodeIds {
  [key: string]: string[];
}

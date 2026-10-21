import { DataCollectionElementType } from '@/constants/aiDataCollection';

export interface IConversationItem {
  id: number;
  title: string;
  isActive?: boolean;
  // Automatic and manual synchronization of table structure
  syncTableStructure?: boolean;
}

export interface IChatAtTableOption {
  tableName: string;
  tableType: DataCollectionElementType;
}

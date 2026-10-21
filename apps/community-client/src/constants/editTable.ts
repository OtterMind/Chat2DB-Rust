export enum EditColumnOperationType { 
  // New
  Add = 'ADD',
  // Modify
  Modify = 'MODIFY',
  // Delete
  Delete = 'DELETE',
}

// nullable
export enum NullableType {
  // Cannot be empty
  NotNull = 0,
  // Can be null
  Null = 1,
}

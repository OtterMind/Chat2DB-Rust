import { TreeNodeType } from '@/constants';
import i18n from '@/i18n';

// Separate the group under treeData
export const separationGroup = (children, treeNodeData, parentId?) => {
  const groupList: any = [];
  children.forEach((item) => {
    if (item.treeNodeType === TreeNodeType.GROUP && treeNodeData.id !== item.id) {
      groupList.push({
        key: item.key,
        id: item.id,
        text: item.originalTitle,
        children: separationGroup(item.children, treeNodeData, item.id),
        parentId,
      });
    }
  });
  return groupList;
};

// Move node to group
export const neatenMoveToGroup = ({ treeData, moveToGroup, treeNodeData }) => {
  const groupList = separationGroup(treeData, treeNodeData);
  const flatList:any = [];

    // Add handle method to each node
  const generatingNode = (item, parentName) => {
    const text = [parentName, item.text].filter(Boolean).join('/');
    return {
      id: item.id,
      key: item.key,
      parentId: item.parentId,
      text,
      handle: () => { 
        moveToGroup({
          dragNode: {
            id: treeNodeData.id,
            type: treeNodeData.treeNodeType === TreeNodeType.GROUP ? 'NAMESPACE' : 'DATA_SOURCE',
          },
          dropToNode: {
            id: item.id,
            type: 'NAMESPACE',
          },
          dropPosition: 2,
        })
      }
    };
  };

  // Flatten the groupList and add a handle method
  const flatGroupList = (children, parentName?) => {
    children.forEach((item) => {
      const node = generatingNode(item,parentName)
      flatList.push(node);
      if (item.children) {
        flatGroupList(item.children, node.text);
      }
     });
  };

  flatGroupList(groupList);


  // root node
  const root = {
    key: 'chat2db_tree_root',
    text: i18n('workspace.tree.root'),
    handle: () => {
      if (treeNodeData.id === treeData[treeData.length - 1].id) {
        return;
      }
      moveToGroup({
        dragNode: {
          id: treeNodeData.id,
          type: treeNodeData.treeNodeType === TreeNodeType.GROUP ? 'NAMESPACE' : 'DATA_SOURCE',
        },
        dropToNode: {
          id: treeData[treeData.length - 1].id,
          type: treeData[treeData.length - 1].treeNodeType === TreeNodeType.GROUP ? 'NAMESPACE' : 'DATA_SOURCE',
        },
        dropPosition: 1,
      });
    },
  }

  // is inserted in the first place
  flatList.unshift(root);

  return flatList;
};

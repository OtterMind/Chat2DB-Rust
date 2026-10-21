// import { useAIStore } from '@/store/ai';
import { useTreeStore } from '@/store/tree';

const useGlobalData = () => {
  // const { getModelList } = useAIStore((state) => ({
  //   getModelList: state.getModelList,
  // }));

  const { getTreeData } = useTreeStore((state) => ({
    getTreeData: state.getTreeData,
  }));

  return () => {
    // getModelList();
    getTreeData();
  };
};

export default useGlobalData;

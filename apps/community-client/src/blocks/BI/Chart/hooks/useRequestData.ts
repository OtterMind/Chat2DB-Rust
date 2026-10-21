// is responsible for the data request logic of each chart, including automatic polling, manual refresh, etc.
import { useEffect, useState } from 'react';

export interface UseRequestDataProps {
  // method of requesting data
  requestData: () => Promise<any>;
  // request interval time
  interval?: number;
}

export const useRequestData = (props: UseRequestDataProps) => {
  const { requestData, interval } = props;
  const [data, setData] = useState<any>();
  const [loading, setLoading] = useState<boolean>(false);

  const fetchData = async () => {
    setLoading(true);
    try {
      const res = await requestData();
      setData(res);
    } catch (error) {
      console.error('请求数据失败', error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
    if (interval) {
      const timer = setInterval(() => {
        fetchData();
      }, interval);
      return () => {
        clearInterval(timer);
      };
    }
  }, []);

  return {
    data,
    loading,
    fetchData,
  };
};

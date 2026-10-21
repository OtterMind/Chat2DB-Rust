const pricingServices = {
  getProductList: async () => [],
  createOrder: async () => {
    throw new Error('Pricing is disabled in community mode');
  },
  getOrder: async () => {
    throw new Error('Pricing is disabled in community mode');
  },
  getOrderList: async () => [],
  cancelSubscription: async () => undefined,
};

export default pricingServices;

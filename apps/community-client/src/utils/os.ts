import jcefApi from '@/jcef';

export const getMacAddress = async () => {
  const macAddresses = await jcefApi?.getMacAddress();

  // Prioritized list of interface names to check
  const interfacePriority = [
    // Windows
    'Ethernet',
    'WLAN',
    'Wi-Fi',
    'Local Area Connection',
    // macOS
    'en0',
    'en1',
    // Linux
    'eth0',
    'wlan0',
    'enp0s3',
    'wlp2s0',
  ];

  for (const interfaceName of interfacePriority) {
    for (const key in macAddresses) {
      if (key.includes(interfaceName)) {
        return macAddresses[key];
      }
    }
  }

  // If no preferred interface is found, return the first available MAC address
  return Object.values(macAddresses)[0] || null;
};

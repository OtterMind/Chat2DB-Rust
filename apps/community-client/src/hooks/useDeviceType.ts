import { useState, useEffect, RefObject } from 'react';

// Define device type
type DeviceType = {
  isPhone: boolean;
  isPad: boolean;
  isPc: boolean;
};

/**
 * Get the auxiliary function of the device type
 * @param width - the width of the current element or window
 * @returns device type object, containing three Boolean values: isPhone, isPad, isPc
 */
function getDeviceType(width: number): DeviceType {
  return {
    isPhone: width < 768,
    isPad: width >= 768 && width < 1024,
    isPc: width >= 1024,
  };
}

/**
 * Custom hook for detecting device type based on the width of a window or specific element.
 *
 * @param ref - optional parameter, a ref object pointing to a DOM element. If this parameter is passed in,
 * The hook will listen for width changes of the element, otherwise it will listen for width changes of the window.
 *
 * @returns returns an object containing three boolean values:
 * - isPhone: indicates that the current device is a mobile phone
 * - isPad: indicates that the current device is a tablet
 * - isPc: indicates that the current device is PC
 *
 * Use the ResizeObserver API to monitor element width changes or window.resize events
 * To monitor window size changes to adapt to the layout needs of different devices.
 */
const useDeviceType = (ref?: RefObject<HTMLElement>): DeviceType => {
  // Initialize device type state, based on the current width of the element or window
  const [deviceType, setDeviceType] = useState<DeviceType>(() => {
    const width = ref?.current ? ref.current.offsetWidth : window.innerWidth;
    return getDeviceType(width);
  });

  useEffect(() => {
    const updateDeviceType = (width: number) => {
      setDeviceType(getDeviceType(width));
    };

    if (ref?.current) {
      // Use ResizeObserver to monitor the width changes of the specified ref
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          updateDeviceType(entry.contentRect.width);
        }
      });

      observer.observe(ref.current);

      // Clean up the observer
      return () => observer.disconnect();
    } else {
      // By default, the width of the window is monitored.
      const handleResize = () => {
        updateDeviceType(window.innerWidth);
      };

      window.addEventListener('resize', handleResize);

      // Clean up event listeners
      return () => window.removeEventListener('resize', handleResize);
    }
  }, [ref]);

  return deviceType;
};

export default useDeviceType;

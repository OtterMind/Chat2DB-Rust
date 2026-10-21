/** Check network connection status */
export async function checkOnlineStatus(appConfig): Promise<boolean> {
  try {
    // /api/version/get_latest_a
    const response = await fetch(`${appConfig.gatewayUrl}/api/version/get_latest_a`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    });

    if (response.ok) {
      return true;
    } else {
      console.warn('Network check failed: Server responded with status', response.status);
      return false;
    }
  } catch (error) {
    console.warn('Network check failed:', error);
    return false;
  }
}

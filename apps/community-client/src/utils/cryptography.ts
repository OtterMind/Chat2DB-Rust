// Define constants
const GCM_NONCE_LENGTH = 12;

export default class CryptographyUtil {
  private accessKey: string;
  private secretKey: string;
  private country: string;
  private timeZone: string;

  constructor(accessKey: string, secretKey: string, country: string, timeZone: string) {
    this.accessKey = accessKey;
    this.secretKey = secretKey;
    this.country = country;
    this.timeZone = timeZone;
  }

  // Get the current time zone in YYMMDDHH format
  private getCurrentTimeZoneFormatted(): string {
    const date = new Date();
    const options = {
      timeZone: this.timeZone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      hour12: false,
    };
    const year = new Intl.DateTimeFormat('en', { ...options, year: 'numeric' }).format(date);
    const month = new Intl.DateTimeFormat('en', { ...options, month: '2-digit' }).format(date);
    const day = new Intl.DateTimeFormat('en', { ...options, day: '2-digit' }).format(date);
    const hour = new Intl.DateTimeFormat('en', { ...options, hour: '2-digit' }).format(date);
    return `${year}${month}${day}${hour}`;
  }

  // Generate message authentication code using HMAC SHA-256 algorithm
  private async hmacSHA256(data: string, key: ArrayBuffer): Promise<ArrayBuffer> {
    const cryptoKey = await crypto.subtle.importKey('raw', key, { name: 'HMAC', hash: { name: 'SHA-256' } }, false, [
      'sign',
    ]);
    const encoder = new TextEncoder();
    const dataBytes = encoder.encode(data);
    return crypto.subtle.sign('HMAC', cryptoKey, dataBytes);
  }

  // Convert ArrayBuffer to hexadecimal string
  private bufferToHex(buffer: ArrayBuffer): string {
    return Array.from(new Uint8Array(buffer))
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
  }

  // SHA-256 hash function
  private async hashSHA256(data: string | null): Promise<string | null> {
    if (data === null) return null;
    const encoder = new TextEncoder();
    const dataBytes = encoder.encode(data);
    const hashBuffer = await crypto.subtle.digest('SHA-256', dataBytes);
    return this.bufferToHex(hashBuffer);
  }

  // Create a canonical request string
  private async createCanonicalRequest(
    httpMethod: string,
    canonicalURI: string,
    canonicalQueryString: string | null,
    payload: string | null,
  ): Promise<string> {
    const hashedPayload = await this.hashSHA256(payload);
    return `${httpMethod}\n${canonicalURI}\n${canonicalQueryString}\n${hashedPayload}`;
  }

  // Create a string to be signed
  private async createStringToSign(canonicalRequest: string): Promise<string> {
    const currentTime = this.getCurrentTimeZoneFormatted();
    const credentialScope = `${currentTime}/${this.country}`;
    const hashedCanonicalRequest = await this.hashSHA256(canonicalRequest);
    return `${currentTime}\n${credentialScope}\n${hashedCanonicalRequest}`;
  }

  // Generate AES key from token
  private async generateAESKeyFromToken(token: string): Promise<CryptoKey> {
    const encoder = new TextEncoder();
    const tokenData = encoder.encode(token);
    const hashedToken = await crypto.subtle.digest('SHA-256', tokenData);
    const keyMaterial = hashedToken.slice(0, 16); // Use first 16 bytes
    return crypto.subtle.importKey('raw', keyMaterial, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
  }

  // AES-GCM encryption
  public async encryptAes(input: string | null): Promise<string | null> {
    if (input === null) return null;
    const key = await this.generateAESKeyFromToken(this.accessKey);
    const encoder = new TextEncoder();
    const inputData = encoder.encode(input);
    const nonce = new Uint8Array(GCM_NONCE_LENGTH);
    const encryptedData = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce, tagLength: 128 }, key, inputData);

    const combined = new Uint8Array(nonce.length + encryptedData.byteLength);
    combined.set(nonce, 0);
    combined.set(new Uint8Array(encryptedData), nonce.length);
    return btoa(String.fromCharCode(...combined));
  }

  // AES-GCM decryption
  public async decryptAes(encryptedValue: string | null): Promise<string | null> {
    if (encryptedValue === null) return null;
    const key = await this.generateAESKeyFromToken(this.accessKey);
    const decoded = atob(encryptedValue);
    const decodedBytes = new Uint8Array(decoded.split('').map((char) => char.charCodeAt(0)));
    const nonce = decodedBytes.slice(0, GCM_NONCE_LENGTH);
    const encryptedBytes = decodedBytes.slice(GCM_NONCE_LENGTH);
    try {
      const decryptedData = await crypto.subtle.decrypt(
        { name: 'AES-GCM', iv: nonce, tagLength: 128 },
        key,
        encryptedBytes,
      );
      return new TextDecoder().decode(decryptedData);
    } catch (exception) {
      console.error('decrypt aes error', exception);
      throw new Error('Decryption error');
    }
  }

  // Derive signing key
  private async deriveSigningKey(date: string): Promise<ArrayBuffer> {
    const kSecret = new TextEncoder().encode('CHAT2DB' + this.secretKey);
    const kDate = await this.hmacSHA256(date, kSecret);
    return this.hmacSHA256(this.country, kDate);
  }

  // Compute signature
  public async calculateSignature(canonicalRequest: string): Promise<string> {
    const stringToSign = await this.createStringToSign(canonicalRequest);
    const currentTime = this.getCurrentTimeZoneFormatted();
    const signingKey = await this.deriveSigningKey(currentTime.substring(0, 8));
    const signatureArrayBuffer = await this.hmacSHA256(stringToSign, signingKey);
    return this.bufferToHex(signatureArrayBuffer);
  }
}

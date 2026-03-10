/** Web Worker for off-thread base64 decoding of large notebook payloads. */

self.onmessage = (event: MessageEvent<string>) => {
  const base64 = event.data;
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  self.postMessage(bytes.buffer, { transfer: [bytes.buffer] });
};

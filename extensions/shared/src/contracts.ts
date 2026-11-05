export const SCHEMA_VERSION = 1;
export const NATIVE_HOST = 'com.grabbit.native';

export interface NativeResponse {
  schemaVersion: number;
  requestId: string | null;
  ok: boolean;
  code: string;
  message: string;
  jobId: string | null;
  data: unknown;
}

export type BrowserKind = 'chrome' | 'brave' | 'edge' | 'firefox' | 'safari';

export function envelope(type: string, payload?: unknown) {
  return { schemaVersion: SCHEMA_VERSION, requestId: crypto.randomUUID(), type, ...(payload === undefined ? {} : { payload }) };
}

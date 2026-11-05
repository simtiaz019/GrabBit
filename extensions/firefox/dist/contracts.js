export const SCHEMA_VERSION = 1;
export const NATIVE_HOST = 'com.grabbit.native';
export function envelope(type, payload) {
    return { schemaVersion: SCHEMA_VERSION, requestId: crypto.randomUUID(), type, ...(payload === undefined ? {} : { payload }) };
}

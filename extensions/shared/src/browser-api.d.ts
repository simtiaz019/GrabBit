declare namespace WebExt {
  interface DownloadItem { id: number; url: string; finalUrl?: string; filename?: string; byExtensionId?: string; incognito?: boolean; referrer?: string; mime?: string; state?: 'in_progress' | 'interrupted' | 'complete'; totalBytes?: number; fileSize?: number }
  interface MenuInfo { menuItemId: string | number; linkUrl?: string; pageUrl?: string; srcUrl?: string }
  interface Tab { id?: number; url?: string }
  interface Api {
    runtime: { sendNativeMessage(host: string, message: unknown): Promise<unknown>; getURL(path: string): string; lastError?: { message?: string } };
    downloads: { onCreated: { addListener(listener: (item: DownloadItem) => void): void }; search(query: Record<string, unknown>): Promise<DownloadItem[]>; cancel(id: number): Promise<void>; erase(query: { id: number }): Promise<number[]> };
    contextMenus: { create(properties: Record<string, unknown>): void; removeAll(): Promise<void>; onClicked: { addListener(listener: (info: MenuInfo, tab?: Tab) => void): void } };
    storage: { local: { get(keys: string[] | string): Promise<Record<string, unknown>>; set(values: Record<string, unknown>): Promise<void>; remove(keys: string[] | string): Promise<void> } };
    tabs: { query(query: Record<string, unknown>): Promise<Tab[]>; create(properties: { url: string }): Promise<Tab> };
    windows: { create(properties: { url: string; type: 'popup'; width: number; height: number }): Promise<unknown> };
    permissions: { contains(permissions: { permissions: string[] }): Promise<boolean> };
  }
}
declare const chrome: WebExt.Api;
declare const browser: WebExt.Api | undefined;

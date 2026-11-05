declare namespace WebExt {
  interface DownloadItem { id: number; url: string; finalUrl?: string; filename?: string; byExtensionId?: string }
  interface MenuInfo { menuItemId: string | number; linkUrl?: string; pageUrl?: string; srcUrl?: string }
  interface Tab { id?: number; url?: string }
  interface Api {
    runtime: { sendNativeMessage(host: string, message: unknown): Promise<unknown>; lastError?: { message?: string } };
    downloads: { onCreated: { addListener(listener: (item: DownloadItem) => void): void }; cancel(id: number): Promise<void>; erase(query: { id: number }): Promise<number[]> };
    contextMenus: { create(properties: Record<string, unknown>): void; removeAll(): Promise<void>; onClicked: { addListener(listener: (info: MenuInfo, tab?: Tab) => void): void } };
    storage: { local: { get(keys: string[] | string): Promise<Record<string, unknown>>; set(values: Record<string, unknown>): Promise<void> } };
    tabs: { query(query: Record<string, unknown>): Promise<Tab[]>; create(properties: { url: string }): Promise<Tab> };
  }
}
declare const chrome: WebExt.Api;
declare const browser: WebExt.Api | undefined;

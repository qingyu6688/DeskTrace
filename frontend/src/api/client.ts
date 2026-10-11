import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { ApiResponse } from '../types/bootstrap';

export class IpcError extends Error {
  public constructor(message: string, public readonly code: number = 503) { super(message); this.name = 'IpcError'; }
}
export function desktopAvailable(): boolean { return isTauri(); }
export async function request<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!desktopAvailable()) throw new IpcError('尚未连接桌面程序，请通过 Tauri 启动 DeskTrace。');
  let response: ApiResponse<T>;
  try { response = await invoke<ApiResponse<T>>(command, args); }
  catch { throw new IpcError('桌面请求失败，请重试或重新打开 DeskTrace。'); }
  if (response.code !== 200) throw new IpcError(response.message, response.code);
  return response.data;
}
export async function subscribe<T>(event: string, callback: (payload: T) => void): Promise<UnlistenFn> {
  if (!desktopAvailable()) return (): void => {};
  try { return await listen<T>(event, (message): void => callback(message.payload)); }
  catch { throw new IpcError('无法订阅状态变化，请重试连接。'); }
}

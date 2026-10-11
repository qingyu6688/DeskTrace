import { invoke, isTauri } from '@tauri-apps/api/core';

import type { ApiResponse, BootstrapStatus } from '../types/bootstrap';

export class ApiError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = 'ApiError';
  }
}

export async function getBootstrapStatus(): Promise<BootstrapStatus | null> {
  if (!isTauri()) return null;

  let response: ApiResponse<BootstrapStatus>;
  try {
    response = await invoke<ApiResponse<BootstrapStatus>>('get_bootstrap_status');
  } catch {
    throw new ApiError('无法连接桌面程序，请重新打开 DeskTrace。');
  }

  if (response.code !== 200) throw new ApiError(response.message);
  return response.data;
}

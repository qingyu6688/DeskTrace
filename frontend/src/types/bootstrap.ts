export type ErrorCode = 400 | 403 | 404 | 409 | 422 | 500 | 503;

export type ApiResponse<T> =
  | { code: 200; message: string; data: T }
  | { code: ErrorCode; message: string; data: null };

export interface DatabaseSummary {
  schemaVersion: number;
  objectCount: number;
  eventCount: number;
  dataDirectory: string;
}

export interface BootstrapStatus {
  appVersion: string;
  platformSupported: boolean;
  collectorImplemented: boolean;
  database: DatabaseSummary;
  dedupWindowSeconds: number;
  intervalReasonCount: number;
}

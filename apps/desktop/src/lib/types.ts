export type BodyKind = "json" | "html" | "text" | "image" | "pdf" | "binary";

export type BodyPreview = {
  kind: BodyKind;
  content_type: string | null;
  size_bytes: number;
  exceeds_threshold: boolean;
  json: unknown | null;
  text: string | null;
};

export type CookieDto = {
  name: string;
  value: string;
  domain: string | null;
  path: string | null;
  expires: string | null;
  max_age: string | null;
  secure: boolean;
  http_only: boolean;
  same_site: string | null;
};

export type SendResponseResult = {
  status: number;
  status_text: string;
  headers: Record<string, string>;
  body: BodyPreview;
  cookies: CookieDto[];
  elapsed_ms: number;
  resolved_url: string;
  history_id: number;
};

export type HistoryEntrySummary = {
  id: number;
  request_label: string;
  method: string;
  url: string;
  status: number;
  status_text: string;
  elapsed_ms: number;
  sent_at: number;
  size_bytes: number;
};

export type HistoryEntryDetail = {
  id: number;
  method: string;
  url: string;
  status: number;
  status_text: string;
  headers: Record<string, string>;
  cookies: CookieDto[];
  body: BodyPreview;
  elapsed_ms: number;
  sent_at: number;
};

export type DiffStatus = "added" | "removed" | "changed" | "unchanged";

export type DiffNode = {
  key: string | null;
  status: DiffStatus;
  old_value: unknown | null;
  new_value: unknown | null;
  children: DiffNode[];
};

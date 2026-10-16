import { invoke } from "@tauri-apps/api/core";

interface UploadConfig {
  base_url: string;
  token: string;
  input_dir: string;
  mode: string;
  /** The Import Run the Upload posts into; the web app starts and completes it. */
  import_id: number;
}

export interface UploadFinishedReport {
  ok: boolean;
  /** The cancel flag paused the Upload: the run resumes from it, so it is not a failure. */
  cancelled: boolean;
  /**
   * The server refused the session the Upload sent (it expired or was ended),
   * which paused the Upload. The window ends the session too.
   */
  session_refused: boolean;
  messages_attempted: number;
  messages_inserted: number;
  messages_deduped: number;
  messages_failed: number;
  assets_uploaded: number;
  assets_bytes: number;
  conversations_ok: number;
  conversations_total: number;
  conversations_failed: number;
  conversations_skipped: number;
  /** Conversations a pause left unsent; the next Upload sends them. */
  conversations_cancelled: number;
  results: Array<{
    file: string;
    status: string;
    error?: string;
    messages: number;
    attachments: number;
  }>;
}

/** Upload extracted conversations to a server. */
export async function invokeUpload(config: UploadConfig): Promise<void> {
  return invoke("upload", {
    args: {
      baseUrl: config.base_url,
      token: config.token,
      inputDir: config.input_dir,
      mode: config.mode,
      importId: config.import_id,
    },
  });
}

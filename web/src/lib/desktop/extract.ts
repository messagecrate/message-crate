import { invoke } from "@tauri-apps/api/core";
import type { ExtractConfig } from "../types";

/** Start extracting a phone backup on the desktop backend. */
export async function invokeExtract(config: ExtractConfig): Promise<void> {
  return invoke("extract", {
    args: {
      source: config.source,
      path: config.path,
      outputDir: config.output_dir,
      backupPassword: config.backup_password ?? null,
      attachmentMedia: config.attachment_media ?? null,
      mediaMaxResolution: config.media_max_resolution ?? null,
      mediaMaxFps: config.media_max_fps ?? null,
      mediaMinSize: config.media_min_size ?? null,
      obfuscate: config.obfuscate ?? null,
      timezone: config.timezone ?? null,
      phoneCountry: config.phone_country ?? null,
      ownerPhones: config.owner_phones ?? null,
      ownerEmails: config.owner_emails ?? null,
      attachmentRoot: config.attachment_root ?? null,
      appleContacts: config.apple_contacts ?? null,
      whatsappKey: config.whatsapp_key ?? null,
      whatsappWa: config.whatsapp_wa ?? null,
      whatsappMedia: config.whatsapp_media ?? null,
      whatsappDb: config.whatsapp_db ?? null,
      isBusinessApp: config.is_business_app ?? null,
      resume: config.resume ?? null,
      assetMaxBytes: config.asset_max_bytes,
    },
  });
}

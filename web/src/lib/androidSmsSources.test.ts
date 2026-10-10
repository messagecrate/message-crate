import { describe, expect, it } from "vitest";
import {
  backupField,
  GO_SMS_PRO_SOURCE,
  SMS_BACKUP_PLUS_SOURCE,
  SMS_BACKUP_RESTORE_SOURCE,
} from "./androidSmsSources";

describe("backupField", () => {
  // An Import Run reads one backup, and SMS Backup & Restore writes each
  // backup as one file: a directory picker would let a run take several.
  it("asks SMS Backup & Restore for one .xml file", () => {
    const field = backupField(SMS_BACKUP_RESTORE_SOURCE);
    expect(field.directory).toBe(false);
    expect(field.filters).toEqual([{ name: "SMS Backup & Restore", extensions: ["xml"] }]);
  });

  it("asks GO SMS Pro and SMS Backup+ for a directory", () => {
    for (const source of [GO_SMS_PRO_SOURCE, SMS_BACKUP_PLUS_SOURCE]) {
      const field = backupField(source);
      expect(field.directory, source).toBe(true);
      expect(field.filters, source).toBeUndefined();
    }
  });
});

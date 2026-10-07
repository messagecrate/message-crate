import {
  commitContactsFromVcf,
  previewContactsFromVcf,
  type VcfCategoryMapping,
} from "@/lib/contactsVcfImport";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

async function readVcfFile(form: FormData): Promise<
  | { ok: true; text: string }
  | { ok: false; response: NextResponse }
> {
  const file = form.get("file");
  if (!(file instanceof File)) {
    return {
      ok: false,
      response: NextResponse.json(
        { error: "file field required (.vcf)" },
        { status: 400 },
      ),
    };
  }

  const name = file.name.toLowerCase();
  if (name && !name.endsWith(".vcf") && !name.endsWith(".vcard")) {
    return {
      ok: false,
      response: NextResponse.json(
        { error: "file must be a .vcf or .vcard" },
        { status: 400 },
      ),
    };
  }

  const maxBytes = 8 * 1024 * 1024;
  if (file.size > maxBytes) {
    return {
      ok: false,
      response: NextResponse.json(
        { error: "VCF file too large (max 8 MB)" },
        { status: 400 },
      ),
    };
  }

  let text: string;
  try {
    text = await file.text();
  } catch {
    return {
      ok: false,
      response: NextResponse.json(
        { error: "failed to read file" },
        { status: 400 },
      ),
    };
  }

  if (!text.trim()) {
    return {
      ok: false,
      response: NextResponse.json({ error: "empty VCF file" }, { status: 400 }),
    };
  }

  return { ok: true, text };
}

function parseMappings(raw: FormDataEntryValue | null): VcfCategoryMapping[] {
  if (raw == null || typeof raw !== "string" || !raw.trim()) {
    return [];
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("mappings must be valid JSON");
  }
  if (!Array.isArray(parsed)) {
    throw new Error("mappings must be an array");
  }
  return parsed.map((item, i) => {
    if (!item || typeof item !== "object") {
      throw new Error(`mappings[${i}] must be an object`);
    }
    const row = item as Record<string, unknown>;
    const source = typeof row.source === "string" ? row.source : "";
    const target = typeof row.target === "string" ? row.target : source;
    const enabled = row.enabled !== false;
    if (!source.trim()) {
      throw new Error(`mappings[${i}].source is required`);
    }
    return { source, target, enabled };
  });
}

/**
 * POST multipart:
 * - `file` (.vcf) required
 * - `mode` = `preview` (default) | `commit`
 * - `mappings` = JSON array of { source, target, enabled } (required for commit)
 */
export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let form: FormData;
  try {
    form = await req.formData();
  } catch {
    return NextResponse.json(
      { error: "expected multipart form data" },
      { status: 400 },
    );
  }

  const fileResult = await readVcfFile(form);
  if (!fileResult.ok) return fileResult.response;

  const modeRaw = form.get("mode");
  const mode =
    typeof modeRaw === "string" && modeRaw.trim()
      ? modeRaw.trim().toLowerCase()
      : "preview";

  if (mode !== "preview" && mode !== "commit") {
    return NextResponse.json(
      { error: "mode must be preview or commit" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      if (mode === "preview") {
        const preview = previewContactsFromVcf(fileResult.text);
        return NextResponse.json(preview);
      }

      const mappings = parseMappings(form.get("mappings"));
      const summary = commitContactsFromVcf(fileResult.text, mappings);
      return NextResponse.json(summary);
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "import failed";
    return NextResponse.json(
      { error: message },
      { status: 500 },
    );
  }
}

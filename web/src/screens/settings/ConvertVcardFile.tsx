import { useRef, useState } from "react";
import Button from "../../components/Button";
import PhoneCountrySelect from "../../components/PhoneCountrySelect";
import { errorLine } from "../../lib/apiErrorMessage";
import { usePhoneCountries } from "../../lib/phoneCountries";
import { saveFile } from "../../lib/saveFile";
import { convertVcardToAddressBook } from "../../lib/serverApi";

/** Largest vCard file the server converts, mirrored here so the refusal is immediate. */
const MAX_BYTES = 64 * 1024 * 1024;

/** The name the converted address book is saved under, the one Export uses. */
const FILE_NAME = "address-book.csv";

/**
 * Convert a vCard file, such as a phone's or Google Contacts' export, into
 * the address book CSV. The person edits the file in a spreadsheet and loads
 * it with Load beside this. Nothing goes into the account here.
 *
 * The country reads each number written without its `+` code, as the import
 * form's Phone's country does. A number that is not valid there stays as the
 * card has it.
 */
export function ConvertVcardFile() {
  const { countries } = usePhoneCountries();
  const fileRef = useRef<HTMLInputElement>(null);
  const [country, setCountry] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);

  const convert = async (file: File) => {
    setBusy(true);
    setError("");
    setSaved(false);
    try {
      if (!file.name.trim().toLowerCase().endsWith(".vcf")) {
        setError("Choose a .vcf file: the contacts a phone or Google Contacts exports.");
        return;
      }
      if (file.size > MAX_BYTES) {
        setError("That file is larger than 64 MB.");
        return;
      }
      // `file.text()` would turn bytes that are not UTF-8 into U+FFFD, so a
      // name in another encoding would arrive changed with no word.
      let text: string;
      try {
        text = new TextDecoder("utf-8", { fatal: true }).decode(await file.arrayBuffer());
      } catch {
        setError("That file is not UTF-8 text. Save it from the contacts app as UTF-8.");
        return;
      }
      const csv = await convertVcardToAddressBook(text, country || null);
      setSaved(await saveFile(FILE_NAME, new Blob([csv], { type: "text/csv" })));
    } catch (err) {
      setError(errorLine(err, "Could not convert that vCard file."));
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  return (
    <div className="mt-5">
      <h4 className="m-0 mb-1 text-[0.813rem] font-medium">Convert a vCard file</h4>
      <p className="m-0 mb-3 text-[0.813rem] text-muted">
        Turn the .vcf file a phone or Google Contacts exports into an address book. Edit it in a
        spreadsheet, then load it above.
      </p>
      <div className="mb-3 max-w-xs">
        <p className="m-0 mb-1 text-[0.813rem] font-medium">Country for numbers saved without +</p>
        <PhoneCountrySelect
          value={country}
          countries={countries}
          onChange={setCountry}
          label="Country for numbers saved without +"
          isDisabled={busy}
        />
      </div>
      <input
        ref={fileRef}
        type="file"
        accept=".vcf,text/vcard,text/x-vcard"
        className="hidden"
        aria-label="vCard file"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) void convert(file);
        }}
      />
      <Button type="button" disabled={busy} onClick={() => fileRef.current?.click()}>
        {busy ? "Converting…" : "Convert a vCard file"}
      </Button>
      {saved ? (
        <p role="status" className="m-0 mt-3 text-[0.813rem]">
          Saved {FILE_NAME}. Edit it, then load it above.
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="m-0 mt-3 text-[0.813rem] text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}

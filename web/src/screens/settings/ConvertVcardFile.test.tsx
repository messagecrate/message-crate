/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { saveFile } from "../../lib/saveFile";
import { convertVcardToAddressBook } from "../../lib/serverApi";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { nth } from "../../test/nth";
import { setupUser } from "../../test/user";
import { ConvertVcardFile } from "./ConvertVcardFile";

vi.mock("../../lib/serverApi", () => ({ convertVcardToAddressBook: vi.fn() }));
vi.mock("../../lib/saveFile", () => ({ saveFile: vi.fn() }));
vi.mock("../../lib/phoneCountries", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/phoneCountries")>()),
  usePhoneCountries: () => ({
    countries: [
      { code: "GB", calling_code: "44", name: "United Kingdom" },
      { code: "US", calling_code: "1", name: "United States" },
    ],
    loading: false,
    error: null,
  }),
}));

const convert = vi.mocked(convertVcardToAddressBook);
const save = vi.mocked(saveFile);

const CARD = "BEGIN:VCARD\nVERSION:3.0\nFN:Ada\nTEL:(555) 555-0100\nEND:VCARD\n";
const BOOK =
  "contact_id,display_name,service,identity,groups\ncard1,Ada,Text Message,'+15555550100,\n";

function chooseFile(name: string, body: string) {
  const input = screen.getByLabelText("vCard file") as HTMLInputElement;
  fireEvent.change(input, { target: { files: [new File([body], name)] } });
}

describe("ConvertVcardFile", () => {
  beforeEach(() => {
    startFakeClock();
    vi.clearAllMocks();
    convert.mockResolvedValue(BOOK);
    save.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
    stopFakeClock();
  });

  it("sends the file with no country until one is picked, and saves the address book", async () => {
    render(<ConvertVcardFile />);
    expect(screen.getByLabelText("vCard file").getAttribute("accept")).toBe(
      ".vcf,text/vcard,text/x-vcard",
    );
    chooseFile("contacts.vcf", CARD);

    await settle();
    expect(convert).toHaveBeenCalledWith(CARD, null);
    expect(save).toHaveBeenCalledTimes(1);
    const [name, file] = nth(save.mock.calls, 0);
    expect(name).toBe("address-book.csv");
    expect(file.type).toBe("text/csv");
    expect(await file.text()).toBe(BOOK);
    expect(screen.getByRole("status").textContent).toContain("Saved address-book.csv");
  });

  it("reads numbers without + in the country picked", async () => {
    const user = setupUser();
    render(<ConvertVcardFile />);
    await user.click(screen.getByRole("button", { name: /Country for numbers saved without/ }));
    await user.click(screen.getByRole("option", { name: "United States (+1)" }));
    chooseFile("Contacts.VCF", CARD);

    await settle();
    expect(convert).toHaveBeenCalledWith(CARD, "US");
  });

  it("takes a .vcf file only, and refuses any other without asking the server", async () => {
    render(<ConvertVcardFile />);
    chooseFile("address-book.csv", "contact_id\n");

    await settle();
    expect(screen.getByRole("alert").textContent).toContain("Choose a .vcf file");
    expect(convert).not.toHaveBeenCalled();
  });

  it("refuses a file that is not UTF-8 without asking the server", async () => {
    render(<ConvertVcardFile />);
    const input = screen.getByLabelText("vCard file") as HTMLInputElement;
    // "FN:M\xfcller" in Windows-1252, as Outlook writes it.
    const bytes = new Uint8Array([0x46, 0x4e, 0x3a, 0x4d, 0xfc, 0x6c, 0x6c, 0x65, 0x72]);
    fireEvent.change(input, { target: { files: [new File([bytes], "outlook.vcf")] } });

    await settle();
    expect(screen.getByRole("alert").textContent).toContain("not UTF-8");
    expect(convert).not.toHaveBeenCalled();
  });

  it("says why when the server refuses the file, and saves nothing", async () => {
    convert.mockRejectedValue(
      new ApiError(422, "the file holds no vCard: it has no BEGIN:VCARD line", {
        type: "https://messagecrate.app/docs/developer/reference/errors/validation-failed",
        title: "Validation failed",
        status: 422,
        errors: ["the file holds no vCard: it has no BEGIN:VCARD line"],
      }),
    );
    render(<ConvertVcardFile />);
    chooseFile("empty.vcf", "nothing here\n");

    await settle();
    expect(screen.getByRole("alert").textContent).toContain("no BEGIN:VCARD line");
    expect(save).not.toHaveBeenCalled();
  });
});

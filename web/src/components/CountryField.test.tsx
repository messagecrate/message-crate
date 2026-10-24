/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type PhoneCountryChoice, searchPhoneCountries } from "../lib/phoneCountries";
import { fill, setupUser } from "../test/user";
import CountryField from "./CountryField";

const COUNTRIES: PhoneCountryChoice[] = [
  { code: "CA", calling_code: "1", name: "Canada" },
  { code: "CD", calling_code: "243", name: "Democratic Republic of the Congo" },
  { code: "DE", calling_code: "49", name: "Germany" },
  { code: "NG", calling_code: "234", name: "Nigeria" },
  { code: "GB", calling_code: "44", name: "United Kingdom" },
  { code: "US", calling_code: "1", name: "United States" },
];

function field() {
  return screen.getByRole("combobox", { name: "Default country for contacts" }) as HTMLInputElement;
}

function shownRows(): string[] {
  return within(screen.getByRole("listbox"))
    .queryAllByRole("option")
    .map((o) => o.textContent ?? "");
}

describe("searchPhoneCountries", () => {
  it.each([
    ["ger", ["DE", "NG"]],
    ["de", ["DE", "CD"]],
    ["GERMANY", ["DE"]],
    ["+49", ["DE"]],
    ["49", ["DE"]],
    ["4", ["DE", "GB"]],
    ["+1", ["CA", "US"]],
    ["DE", ["DE", "CD"]],
    ["gb", ["GB"]],
    ["united", ["GB", "US"]],
    ["", ["CA", "CD", "DE", "NG", "GB", "US"]],
    ["xyz", []],
  ])("finds %j as %j", (query, codes) => {
    expect(searchPhoneCountries(COUNTRIES, query).map((c) => c.code)).toEqual(codes);
  });
});

describe("CountryField", () => {
  afterEach(cleanup);

  function renderField(value: string, onChange = vi.fn(), allowsNone = true) {
    render(
      <CountryField
        value={value}
        countries={COUNTRIES}
        onChange={onChange}
        label="Default country for contacts"
        allowsNone={allowsNone}
      />,
    );
    return onChange;
  }

  it("shows None as the empty field's placeholder and as the first row", async () => {
    const user = setupUser();
    renderField("");
    expect(field().value).toBe("");
    expect(field().placeholder).toBe("None");
    await user.click(field());
    expect(shownRows()).toEqual([
      "None",
      "Canada (+1)",
      "Democratic Republic of the Congo (+243)",
      "Germany (+49)",
      "Nigeria (+234)",
      "United Kingdom (+44)",
      "United States (+1)",
    ]);
  });

  it("shows the picked country by its name and calling code", () => {
    renderField("GB");
    expect(field().value).toBe("United Kingdom (+44)");
  });

  it.each([
    ["germ", "Germany (+49)"],
    ["+49", "Germany (+49)"],
    ["49", "Germany (+49)"],
    ["DE", "Germany (+49)"],
  ])(
    "narrows the rows to what %j finds, and hands back the pick's ISO code",
    async (typed, row) => {
      const user = setupUser();
      const onChange = renderField("");
      await fill(user, field(), typed);
      expect(shownRows()[0]).toBe(row);
      await user.click(screen.getByRole("option", { name: row }));
      expect(onChange).toHaveBeenCalledWith("DE");
    },
  );

  it("hands back an empty code when None is picked", async () => {
    const user = setupUser();
    const onChange = renderField("GB");
    await user.click(field());
    await user.click(screen.getByRole("option", { name: "None" }));
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("says so when nothing matches, and puts the choice back when the person leaves", async () => {
    const user = setupUser();
    const onChange = renderField("GB");
    await fill(user, field(), "xyz");
    expect(screen.getByText("No country matches")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(field().value).toBe("United Kingdom (+44)");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("has no None row when the field must name a country", async () => {
    const user = setupUser();
    renderField("", vi.fn(), false);
    await user.click(field());
    expect(shownRows()).not.toContain("None");
    expect(shownRows()).toHaveLength(COUNTRIES.length);
  });
});

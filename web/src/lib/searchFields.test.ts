import { describe, expect, it } from "vitest";
import { searchFieldsFor } from "../test/searchFields";
import { hasFieldToken, markedWords, stripFieldTokens } from "./searchFields";

describe("hasFieldToken", () => {
  it("is true for any word: token, negated or not", () => {
    expect(hasFieldToken("group:Family")).toBe(true);
    expect(hasFieldToken("ana -tag:Work")).toBe(true);
    expect(hasFieldToken('title:"book club"')).toBe(true);
  });
  it("sees a token that a bracket opens", () => {
    expect(hasFieldToken("(kind:group or kind:direct)")).toBe(true);
  });
  it("is false for plain words, phrases, and colons inside a phrase", () => {
    expect(hasFieldToken("ana")).toBe(false);
    expect(hasFieldToken('"re: dinner"')).toBe(false);
    expect(hasFieldToken("http://example.com")).toBe(false);
    expect(hasFieldToken("")).toBe(false);
  });
});

describe("stripFieldTokens", () => {
  it("keeps the words and drops the tokens", () => {
    expect(stripFieldTokens("ana group:Family")).toBe("ana");
    expect(stripFieldTokens('identity:"+1 555" bo -tag:x')).toBe("bo");
    expect(stripFieldTokens("just words")).toBe("just words");
  });
  it("drops a token a bracket opens too, and keeps the brackets", () => {
    expect(stripFieldTokens("(kind:group or kind:direct)")).toBe("( or )");
  });
  it("keeps a colon inside a quoted phrase", () => {
    expect(stripFieldTokens('"re: dinner" kind:group')).toBe('"re: dinner"');
  });
});

describe("markedWords", () => {
  const marked = (q: string, list: "conversations" | "messages") => {
    const other = list === "messages" ? "conversations" : "messages";
    return markedWords(q, searchFieldsFor(list), { list: other, fields: searchFieldsFor(other) });
  };

  it("marks a word only the other list takes, with the list it works in", () => {
    expect(marked("from:ann hello", "conversations")).toEqual([
      { word: "from", start: 0, end: 8, worksIn: "messages" },
    ]);
    expect(marked("hello messages:>5", "messages")).toEqual([
      { word: "messages", start: 6, end: 17, worksIn: "conversations" },
    ]);
  });

  it("leaves a word the list takes, and one no list has, to the server", () => {
    expect(marked("body:x tag:Work note:x", "conversations")).toEqual([]);
  });
});

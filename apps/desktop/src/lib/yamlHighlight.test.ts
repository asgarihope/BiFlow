import { describe, expect, it } from "vitest";
import { highlightYamlLine } from "./yamlHighlight";

describe("highlightYamlLine", () => {
  it("marks keys, comments, and quoted values", () => {
    expect(highlightYamlLine("# comment")[0]?.kind).toBe("comment");
    expect(highlightYamlLine('secret: "<redacted>"')[0]).toEqual({
      kind: "key",
      text: "secret",
    });
    expect(highlightYamlLine('secret: "<redacted>"')[1]?.kind).toBe("string");
    expect(highlightYamlLine("- MATCH,DIRECT")[0]?.kind).toBe("plain");
  });
});

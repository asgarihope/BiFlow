export type YamlTokenKind = "plain" | "key" | "string" | "comment";

export interface YamlToken {
  kind: YamlTokenKind;
  text: string;
}

/** Colors one YAML line for the read-only Mihomo config view. */
export function highlightYamlLine(line: string): YamlToken[] {
  const trimmed = line.trimStart();
  if (trimmed.startsWith("#")) {
    return [{ kind: "comment", text: line }];
  }
  const colon = line.indexOf(":");
  if (colon <= 0) {
    return [{ kind: "plain", text: line }];
  }
  const key = line.slice(0, colon);
  const rest = line.slice(colon);
  return [
    { kind: "key", text: key },
    {
      kind: rest.includes('"') || rest.includes("'") ? "string" : "plain",
      text: rest,
    },
  ];
}

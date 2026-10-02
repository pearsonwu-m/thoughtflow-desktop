import { describe, expect, it } from "vitest";
import { parseModel, supportsEffort, supportsTemperature } from "./models";

describe("model capabilities", () => {
  it("parses ids like the backend", () => {
    expect(parseModel("claude-opus-5-5")).toEqual({ family: "opus", version: [5, 5] });
    expect(parseModel("claude-haiku-4-5-20251001")).toEqual({ family: "haiku", version: [4, 5] });
    expect(parseModel("claude-3-7-sonnet-20250219")).toEqual({ family: "sonnet", version: [3, 7] });
  });

  it("only offers temperature where the API accepts it", () => {
    expect(supportsTemperature("claude-opus-5-5")).toBe(false);
    expect(supportsTemperature("claude-sonnet-5-5")).toBe(false);
    expect(supportsTemperature("claude-haiku-4-5")).toBe(true);
    expect(supportsTemperature("claude-opus-4-6")).toBe(true);
  });

  it("only offers depth where the API accepts it", () => {
    expect(supportsEffort("claude-opus-5-5")).toBe(true);
    expect(supportsEffort("claude-haiku-4-5")).toBe(false);
    expect(supportsEffort("my-custom-model")).toBe(false);
  });
});

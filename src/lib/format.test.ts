import { describe, expect, it } from "vitest";
import { dayLabel, formatBytes, formatDue, groupByDay, isoDate, modelLabel } from "./format";

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime();

describe("dayLabel", () => {
  const now = at(2026, 10, 1, 21);
  it("labels recent days", () => {
    expect(dayLabel(at(2026, 10, 1, 8), now)).toBe("Today");
    expect(dayLabel(at(2026, 9, 30, 23), now)).toBe("Yesterday");
    expect(dayLabel(at(2026, 9, 28), now)).toMatch(/Monday/);
  });
});

describe("groupByDay", () => {
  it("groups consecutive items under one heading", () => {
    const now = at(2026, 10, 1, 21);
    const groups = groupByDay(
      [{ updatedAt: at(2026, 10, 1, 21) }, { updatedAt: at(2026, 10, 1, 9) }, { updatedAt: at(2026, 9, 30) }],
      now,
    );
    expect(groups.map((g) => [g.label, g.items.length])).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
    ]);
  });
});

describe("dates", () => {
  it("formats ISO dates and due hints", () => {
    const now = new Date(2026, 9, 1, 10);
    expect(isoDate(now)).toBe("2026-10-01");
    expect(formatDue("2026-10-01", now)).toBe("today");
    expect(formatDue("2026-10-02", now)).toBe("tomorrow");
    expect(formatDue("2026-09-20", now)).toMatch(/^overdue/);
    expect(formatDue(null, now)).toBeNull();
  });
});

describe("labels", () => {
  it("formats model ids and sizes", () => {
    expect(modelLabel("claude-opus-5-5")).toBe("Claude Opus 5.5");
    expect(modelLabel("claude-haiku-4-5-20251001")).toBe("Claude Haiku 4.5");
    expect(modelLabel("claude-opus-5")).toBe("Claude Opus 5");
    expect(modelLabel("custom-model")).toBe("custom-model");
    expect(formatBytes(2048)).toBe("2.0 KB");
  });
});

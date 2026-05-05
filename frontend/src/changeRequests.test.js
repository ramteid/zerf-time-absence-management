import { describe, expect, it } from "vitest";
import { buildChangeRequestPayload } from "./changeRequests.js";

const entry = {
  id: 42,
  entry_date: "2026-05-05",
  start_time: "08:00",
  end_time: "12:00",
  category_id: 3,
  comment: "initial note",
};

describe("buildChangeRequestPayload", () => {
  it("rejects a request without an actual edit", () => {
    const result = buildChangeRequestPayload(entry, {
      entry_date: "2026-05-05",
      start_time: "08:00",
      end_time: "12:00",
      category_id: 3,
      comment: "initial note",
      reason: "please fix",
    });

    expect(result).toEqual({ error: "Please change at least one field." });
  });

  it("builds a payload for changed fields only", () => {
    const result = buildChangeRequestPayload(entry, {
      entry_date: "2026-05-06",
      start_time: "09:00",
      end_time: "13:00",
      category_id: 4,
      comment: "shifted work",
      reason: "schedule changed",
    });

    expect(result).toEqual({
      payload: {
        time_entry_id: 42,
        reason: "schedule changed",
        new_date: "2026-05-06",
        new_start_time: "09:00",
        new_end_time: "13:00",
        new_category_id: 4,
        new_comment: "shifted work",
      },
    });
  });

  it("supports clearing an existing comment", () => {
    const result = buildChangeRequestPayload(entry, {
      entry_date: "2026-05-05",
      start_time: "08:00",
      end_time: "12:00",
      category_id: 3,
      comment: "",
      reason: "remove comment",
    });

    expect(result).toEqual({
      payload: {
        time_entry_id: 42,
        reason: "remove comment",
        new_comment: "",
      },
    });
  });
});
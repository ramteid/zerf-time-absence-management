function normalizeCommentValue(value) {
  return value === "" || value == null ? null : value;
}

export function buildChangeRequestPayload(entry, draft) {
  const reason = String(draft.reason || "").trim();
  if (!reason) {
    return { error: "Reason required" };
  }
  if (!draft.entry_date) {
    return { error: "Invalid date." };
  }
  if (draft.start_time >= draft.end_time) {
    return { error: "Start cannot be after End." };
  }
  if (draft.category_id == null) {
    return { error: "Category required." };
  }

  const payload = {
    time_entry_id: entry.id,
    reason,
  };

  if (draft.entry_date !== entry.entry_date) {
    payload.new_date = draft.entry_date;
  }
  if (draft.start_time !== (entry.start_time?.slice(0, 5) || "")) {
    payload.new_start_time = draft.start_time;
  }
  if (draft.end_time !== (entry.end_time?.slice(0, 5) || "")) {
    payload.new_end_time = draft.end_time;
  }
  if (Number(draft.category_id) !== Number(entry.category_id)) {
    payload.new_category_id = Number(draft.category_id);
  }

  const nextComment = normalizeCommentValue(draft.comment);
  const currentComment = normalizeCommentValue(entry.comment);
  if (nextComment !== currentComment) {
    payload.new_comment = draft.comment;
  }

  if (Object.keys(payload).length === 2) {
    return { error: "Please change at least one field." };
  }

  return { payload };
}
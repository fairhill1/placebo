for (const type of ["scheduled", "request", "applied", "discarded", "deferred", "ignored", "error"]) {
  document.addEventListener(`placebo:${type}`, ({ detail }) => {
    const trace = document.getElementById("trace");
    const entry = document.createElement("li");
    entry.className = type;
    const what = detail.action ? `${detail.action} → ${detail.target}` : `read ${detail.path ?? "page"}`;
    entry.textContent = `${type} · ${what}${detail.outcome ? ` · ${detail.outcome}` : ""}${detail.reason ? ` · ${detail.reason}` : ""}${detail.code ? ` · ${detail.code}: ${detail.message}` : ""}`;
    trace.prepend(entry);
    while (trace.children.length > 30) trace.lastElementChild.remove();
    if (type === "error") trace.closest("details").open = true;
  });
}

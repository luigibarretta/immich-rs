"use strict";

const panel = document.querySelector("[data-events-url]");
if (panel && panel.dataset.terminal !== "true" && "EventSource" in window) {
  const source = new EventSource(panel.dataset.eventsUrl);
  const fallback = document.getElementById("polling-fallback");
  const stages = {
    discovery: "Discovering entries",
    identity: "Calculating content identity",
    reconciliation: "Reconciling metadata",
    complete: "Finalizing plan",
    starting: "Starting",
  };
  source.addEventListener("job", (message) => {
    const fields = message.data.split("|");
    if (fields.length !== 12 || !Object.hasOwn(stages, fields[1])) {
      source.close();
      fallback.hidden = false;
      return;
    }
    document.getElementById("job-status").textContent = `${fields[0]} · ${stages[fields[1]]}`;
    document.getElementById("assets-observed").textContent = fields[3];
    document.getElementById("progress-bytes").textContent = fields[4];
    if (["Completed", "Failed", "Cancelled"].includes(fields[0])) {
      source.close();
      window.location.reload();
    }
  });
  source.addEventListener("replay-exhausted", () => {
    source.close();
    fallback.hidden = false;
  });
  source.onerror = () => {
    source.close();
    fallback.hidden = false;
  };
}

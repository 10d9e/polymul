"use strict";

const $ = (sel) => document.querySelector(sel);

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]
  ));
}

function renderNote(text) {
  if (!text) return "";
  let html = escapeHtml(text);
  html = html.replace(/`([^`]+)`/g, "<code>$1</code>");
  html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  html = html.split("\n").join("<br>");
  return html;
}

function fmt(n) {
  return n == null ? "—" : n.toLocaleString("en-US");
}

function fmtCompact(n) {
  if (n == null) return "—";
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "G";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  return fmt(n);
}

function statCard(label, value, opts = {}) {
  const cls = opts.good ? "value good" : "value";
  const sub = opts.sub ? `<div class="sub">${opts.sub}</div>` : "";
  return `<div class="stat"><div class="label">${label}</div><div class="${cls}">${value}</div>${sub}</div>`;
}

function renderStats(data) {
  const scored = data.entries.filter((e) => e.score != null);
  const record = data.record ? data.record.score : null;
  const baseline = data.baseline;
  const improvement = baseline != null && record != null ? baseline - record : null;
  const pct = improvement != null && baseline ? ((improvement / baseline) * 100).toFixed(2) : null;

  $("#stats").innerHTML = [
    statCard("Current record", fmtCompact(record), {
      good: true,
      sub: data.record ? `${data.record.author} · #${data.record.id}` : "",
    }),
    statCard("Baseline", fmtCompact(baseline), { sub: "entry #0001" }),
    statCard("Speedup", improvement != null ? fmtCompact(improvement) : "—", {
      good: improvement != null && improvement > 0,
      sub: pct != null ? `${pct}% less WORK` : "",
    }),
    statCard("Submissions", String(scored.length)),
  ].join("");
}

function runningRecordFrontier(entries) {
  let best = Infinity;
  return entries.map((e) => {
    if (e.score == null) return null;
    if (e.score < best) best = e.score;
    return best === Infinity ? null : best;
  });
}

let CHART = null;
let CHART_STATE = null;

function renderChart(data) {
  const scored = data.entries.filter((e) => e.score != null);
  const labels = scored.map((e) => `#${e.id}`);
  const scores = scored.map((e) => e.score);
  const frontier = runningRecordFrontier(scored);
  const styles = scored.map((e) => ({
    radius: e.isRecord ? 5 : 3,
    bg: e.isRecord ? "#22d3ee" : "rgba(255,255,255,0.5)",
    border: "#000",
  }));

  // Module-level state so the range slider can re-slice the series in place.
  CHART_STATE = { scored, labels, scores, frontier, styles, lo: 0, hi: scored.length - 1 };

  const ctx = $("#scoreChart").getContext("2d");
  const grad = ctx.createLinearGradient(0, 0, 0, 320);
  grad.addColorStop(0, "rgba(34, 211, 238, 0.12)");
  grad.addColorStop(1, "rgba(34, 211, 238, 0)");

  CHART = new Chart(ctx, {
    type: "line",
    data: {
      labels: labels.slice(),
      datasets: [
        {
          label: "Best SCORE so far",
          data: frontier.slice(),
          borderColor: "rgba(34, 211, 238, 0.85)",
          backgroundColor: grad,
          fill: true,
          stepped: "before",
          tension: 0,
          borderWidth: 1.5,
          pointRadius: 0,
          order: 1,
        },
        {
          label: "Submissions",
          data: scores.slice(),
          showLine: false,
          pointRadius: styles.map((s) => s.radius),
          pointBackgroundColor: styles.map((s) => s.bg),
          pointBorderColor: styles.map((s) => s.border),
          pointBorderWidth: 2,
          order: 0,
        },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: { duration: 250 },
      plugins: {
        legend: {
          display: true,
          position: "bottom",
          labels: {
            color: "rgba(255,255,255,0.45)",
            font: { family: "'DM Mono', monospace", size: 9 },
            filter: (item) => item.text !== "Submissions",
          },
        },
        tooltip: {
          filter: (item) => item.datasetIndex === 1,
          callbacks: {
            title: (items) => {
              const e = CHART_STATE.scored[CHART_STATE.lo + items[0].dataIndex];
              return `#${e.id} · ${e.author}`;
            },
            label: (item) =>
              `SCORE: ${fmt(CHART_STATE.scored[CHART_STATE.lo + item.dataIndex].score)} WORK`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: "rgba(255,255,255,0.05)" },
          ticks: { color: "rgba(255,255,255,0.28)", font: { family: "'DM Mono', monospace", size: 9 } },
        },
        y: {
          grid: { color: "rgba(255,255,255,0.05)" },
          ticks: {
            color: "rgba(255,255,255,0.28)",
            font: { family: "'DM Mono', monospace", size: 9 },
            callback: (v) => fmtCompact(v),
          },
          title: {
            display: true,
            text: "deterministic wasm WORK (lower is faster)",
            color: "rgba(255,255,255,0.22)",
            font: { family: "'DM Mono', monospace", size: 9 },
          },
        },
      },
    },
  });

  setupRangeSlider();
}

// Re-slice the chart to the inclusive index window [lo, hi]; the y-axis
// auto-rescales to the window, which is what makes flat tail regions readable.
function applyRange(lo, hi) {
  if (!CHART || !CHART_STATE) return;
  const st = CHART_STATE;
  st.lo = lo;
  st.hi = hi;
  CHART.data.labels = st.labels.slice(lo, hi + 1);
  CHART.data.datasets[0].data = st.frontier.slice(lo, hi + 1);
  CHART.data.datasets[1].data = st.scores.slice(lo, hi + 1);
  const sl = st.styles.slice(lo, hi + 1);
  CHART.data.datasets[1].pointRadius = sl.map((s) => s.radius);
  CHART.data.datasets[1].pointBackgroundColor = sl.map((s) => s.bg);
  CHART.data.datasets[1].pointBorderColor = sl.map((s) => s.border);
  CHART.update();
}

function setupRangeSlider() {
  const ctrl = $("#rangeControl");
  const minI = $("#rangeMin");
  const maxI = $("#rangeMax");
  const fill = $("#rangeFill");
  const label = $("#rangeLabel");
  const n = CHART_STATE.scored.length;
  if (n < 4) { ctrl.hidden = true; return; } // not worth slicing a tiny series
  ctrl.hidden = false;

  const last = n - 1;
  minI.max = maxI.max = String(last);
  minI.value = "0";
  maxI.value = String(last);

  const update = (ev) => {
    let lo = +minI.value;
    let hi = +maxI.value;
    if (lo > hi) {
      // The thumb that moved pushes the other so they never cross.
      if (ev && ev.target === maxI) { lo = hi; minI.value = String(lo); }
      else { hi = lo; maxI.value = String(hi); }
    }
    // Keep whichever thumb sits at the far end clickable when they overlap.
    minI.style.zIndex = lo > last / 2 ? 5 : 4;
    const pl = (lo / last) * 100;
    const ph = (hi / last) * 100;
    fill.style.left = pl + "%";
    fill.style.right = 100 - ph + "%";
    const s = CHART_STATE.scored;
    label.textContent = `#${s[lo].id} → #${s[hi].id} · ${hi - lo + 1} of ${n}`;
    applyRange(lo, hi);
  };

  minI.addEventListener("input", update);
  maxI.addEventListener("input", update);
  $("#rangeReset").addEventListener("click", () => {
    minI.value = "0";
    maxI.value = String(last);
    update();
  });
  update();
}

let ENTRIES_BY_ID = {};

function renderGrid(data) {
  const total = data.entries.length;
  const leaderId = data.record?.id ?? null;
  $("#entryCount").textContent = `${total} ${total === 1 ? "entry" : "entries"}`;
  ENTRIES_BY_ID = Object.fromEntries(data.entries.map((e) => [e.id, e]));

  const rows = [...data.entries].reverse();
  const body = rows.map((e) => {
    const user = (e.author || "").replace(/^@/, "");
    const avatar = user ? `https://github.com/${encodeURIComponent(user)}.png?size=80` : "";
    const deltaClass = e.isRecord ? "good" : "flat";
    const crown = e.id === leaderId
      ? `<span class="score-crown" title="Current record"><svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor"><path d="M2 19h20v2H2v-2zm2.4-8.2 2.1 2.1 3.5-6.3 3.5 6.3 2.1-2.1L20.6 19H3.4l1-8.2z"/></svg></span>`
      : "";
    return `
      <tr class="${e.isRecord ? "record" : ""}" data-id="${e.id}" tabindex="0">
        <td class="c-id">#${e.id}</td>
        <td class="c-author">${crown}<img class="avatar" src="${avatar}" alt="" onerror="this.style.visibility='hidden'"/><span class="aname">${escapeHtml(e.author)}</span></td>
        <td class="c-model">${escapeHtml(e.model || "—")}</td>
        <td class="c-score">${fmtCompact(e.score)}</td>
        <td class="c-delta"><span class="badge ${deltaClass}">${escapeHtml(e.delta)}</span></td>
        <td class="c-open"><span class="open-btn">View ↗</span></td>
      </tr>`;
  }).join("");

  $("#grid").innerHTML = `
    <colgroup>
      <col class="w-id"/><col class="w-author"/><col class="w-model"/>
      <col class="w-score"/><col class="w-delta"/><col class="w-open"/>
    </colgroup>
    <thead><tr>
      <th>#</th><th>Committer</th><th>Model</th><th>SCORE</th><th>Δ</th><th></th>
    </tr></thead>
    <tbody>${body}</tbody>`;

  $("#grid").querySelectorAll("tbody tr").forEach((tr) => {
    const open = () => openDialog(ENTRIES_BY_ID[tr.dataset.id], data.repo);
    tr.addEventListener("click", open);
    tr.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); open(); }
    });
  });
}

function openDialog(e, repo) {
  if (!e) return;
  const user = (e.author || "").replace(/^@/, "");
  const avatar = user ? `https://github.com/${encodeURIComponent(user)}.png?size=120` : "";
  const profile = user ? `https://github.com/${user}` : "#";
  const commitUrl = `https://github.com/${repo}/commit/${e.commit}`;
  const entryUrl = e.entryPath ? `https://github.com/${repo}/blob/main/${e.entryPath}` : "";

  $("#dialogInner").innerHTML = `
    <button class="dialog-close" aria-label="Close" data-close>×</button>
    <header class="dialog-head">
      <img class="d-avatar" src="${avatar}" alt="" onerror="this.style.visibility='hidden'"/>
      <div>
        <div class="d-title">Entry #${e.id} ${e.isRecord ? '<span class="badge good">record</span>' : ""}</div>
        <div class="d-sub"><a href="${profile}" target="_blank" rel="noopener">${escapeHtml(e.author)}</a> · ${escapeHtml(e.date)}${e.model ? ` · ${escapeHtml(e.model)}` : ""}</div>
      </div>
    </header>
    <div class="d-metrics">
      <div class="d-metric"><span class="m-label">SCORE</span><span class="m-value">${fmt(e.score)}</span></div>
      <div class="d-metric"><span class="m-label">Δ</span><span class="m-value">${escapeHtml(e.delta)}</span></div>
      <div class="d-metric"><span class="m-label">commit</span><span class="m-value"><a href="${commitUrl}" target="_blank">${escapeHtml(e.commit)}</a></span></div>
    </div>
    ${e.approach ? `<section class="d-sec"><h3>Approach</h3><div class="note">${renderNote(e.approach)}</div></section>` : ""}
    ${e.iterationNotes ? `<section class="d-sec"><h3>Iteration notes</h3><div class="note">${renderNote(e.iterationNotes)}</div></section>` : ""}
    ${e.evalSnapshot ? `<section class="d-sec"><h3>Eval snapshot</h3><pre class="snapshot">${escapeHtml(e.evalSnapshot)}</pre></section>` : ""}
    <footer class="dialog-foot">${entryUrl ? `<a href="${entryUrl}" target="_blank" rel="noopener">Full entry on GitHub →</a>` : ""}</footer>`;

  const dlg = $("#entryDialog");
  dlg.querySelector("[data-close]").addEventListener("click", () => dlg.close());
  dlg.showModal();
  if (history.replaceState) history.replaceState(null, "", `#${e.id}`);
}

async function main() {
  try {
    const res = await fetch("./data/leaderboard.json", { cache: "no-cache" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    const repo = data.repo || "10d9e/polymul";

    $("#repoLink").href = `https://github.com/${repo}`;
    if (data.generatedAt) {
      $("#generatedAt").textContent = `Updated ${new Date(data.generatedAt).toLocaleString()}`;
    }

    const instrDlg = $("#instructionsDialog");
    $("#instructionsBtn").addEventListener("click", () => {
      const base = `https://github.com/${repo}`;
      $("#instructionsReadme").href = `${base}/blob/main/AUTORESEARCH.md`;
      $("#instructionsContrib").href = `${base}/blob/main/CONTRIBUTING.md`;
      instrDlg.showModal();
    });
    instrDlg.querySelector("[data-close]").addEventListener("click", () => instrDlg.close());
    instrDlg.addEventListener("click", (ev) => { if (ev.target === instrDlg) instrDlg.close(); });

    renderStats(data);
    renderChart(data);
    renderGrid(data);

    const hashId = location.hash.replace(/^#/, "");
    if (hashId && ENTRIES_BY_ID[hashId]) openDialog(ENTRIES_BY_ID[hashId], repo);
    else {
      try {
        if (localStorage.getItem("polymul-instructions-seen") !== "1") {
          $("#instructionsBtn").click();
          localStorage.setItem("polymul-instructions-seen", "1");
        }
      } catch (_) {}
    }
  } catch (err) {
    document.querySelector("main").innerHTML =
      `<div class="error">Could not load leaderboard.<br><small>${escapeHtml(String(err))}</small></div>`;
  }
}

main();

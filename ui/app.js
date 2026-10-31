// Порядок — интерфейс. Без сборщиков и библиотек: один модуль, который зовёт ядро через Tauri.

const tauri = window.__TAURI__;
const invoke = tauri.core.invoke;
const $ = (id) => document.getElementById(id);
const card = $("card");

const state = { folders: false, busy: false, view: "home", canUndo: false };

// ---------- Мелочи ----------

function plural(n, one, few, many) {
  const a = n % 10, b = n % 100;
  const word = a === 1 && b !== 11 ? one : a >= 2 && a <= 4 && (b < 12 || b > 14) ? few : many;
  return `${n} ${word}`;
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

let toastTimer;
function toast(message) {
  const t = $("toast");
  t.textContent = String(message);
  t.classList.add("is-shown");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("is-shown"), 4200);
}

function show(view) {
  for (const name of ["home", "plan", "done"]) $(name).hidden = name !== view;
  state.view = view;
}

// ---------- Визитка ----------

async function loadStart() {
  const start = await invoke("start");
  const nav = $("choices");
  nav.replaceChildren();
  start.choices.forEach((choice, i) => {
    const b = el("button", "link", choice.label);
    b.type = "button";
    b.title = `${choice.paths.join(", ")}   (клавиша ${i + 1})`;
    b.addEventListener("click", () => planFor(choice.paths));
    nav.append(b);
  });
  state.choices = start.choices;
  state.canUndo = Boolean(start.last);
  const undo = $("undo");
  undo.hidden = !start.last;
  if (start.last) undo.title = `${start.last.folder}: ${start.last.count}`;
  if (start.rules_error) toast(start.rules_error);
}

function renderFoldersToggle() {
  const b = $("folders");
  b.textContent = state.folders ? "папки: двигаю целиком" : "папки: не трогаю";
  b.setAttribute("aria-pressed", String(state.folders));
}

// ---------- План ----------

async function planFor(paths) {
  if (state.busy) return;
  state.busy = true;
  const hint = $("hint");
  const calm = hint.textContent;
  hint.textContent = "смотрю, что где лежит…";
  try {
    const plan = await invoke("make_plan", { paths, folders: state.folders });
    renderPlan(plan);
    show("plan");
    $("shelves").focus();
  } catch (e) {
    toast(e);
  } finally {
    hint.textContent = calm;
    state.busy = false;
  }
}

function shelf({ title, count, where, items, extra = "" }) {
  const node = el("div", `shelf ${extra}`);
  node.tabIndex = 0;
  node.setAttribute("role", "button");
  node.setAttribute("aria-expanded", "false");

  const row = el("div", "shelf__row");
  row.append(el("span", "shelf__title", title), el("span", "shelf__dots"), el("span", "shelf__count num", String(count)));
  const list = el("ul", "shelf__items");
  for (const item of items) list.append(el("li", "", item));
  node.append(row, el("div", "shelf__where", where), list);

  const toggle = () => {
    const open = node.classList.toggle("is-open");
    node.setAttribute("aria-expanded", String(open));
  };
  node.addEventListener("click", (e) => {
    if (!e.target.closest(".shelf__items")) toggle();
  });
  node.addEventListener("keydown", (e) => {
    if (e.key === " ") {
      e.preventDefault();
      toggle();
    }
  });
  return node;
}

function renderPlan(plan) {
  $("plan-folder").textContent = plan.folder;
  $("plan-total").textContent = plan.count ? `${plan.total} · ${plan.size}` : "";
  const box = $("shelves");
  box.replaceChildren();
  $("apply").hidden = plan.count === 0;
  $("status").textContent = "";

  if (plan.count === 0) {
    box.append(el("div", "empty", "Тут уже порядок."));
  }
  for (const s of plan.shelves) {
    box.append(shelf({ title: s.title, count: s.count, where: `${s.place} · ${s.size}`, items: s.items }));
  }
  if (plan.skipped.length) {
    box.append(
      shelf({
        title: "Оставлю на месте",
        count: plan.skipped.length,
        where: "ярлыки, программы, проекты и то, что не узнал",
        items: plan.skipped.map((k) => `${k.name} — ${k.reason}`),
        extra: "shelf--rest",
      }),
    );
  }
  if (plan.cloud) box.append(el("p", "note", plan.cloud));
}

async function applyPlan() {
  if (state.busy || $("apply").hidden) return;
  state.busy = true;
  card.classList.add("is-busy");
  $("status").textContent = "раскладываю";
  const bar = $("progress");
  const stop = await tauri.event.listen("progress", ({ payload }) => {
    bar.style.setProperty("--p", payload.total ? payload.done / payload.total : 0);
    $("status").textContent = `${payload.done} из ${payload.total}`;
  });
  try {
    renderDone(await invoke("apply_plan"), "apply");
  } catch (e) {
    toast(e);
  } finally {
    stop();
    state.busy = false;
    card.classList.remove("is-busy");
    bar.style.setProperty("--p", 0);
  }
}

// ---------- Итог ----------

function renderDone(result, mode) {
  $("result").textContent = result.text;
  const failed = result.failed.length;
  const note =
    failed > 0
      ? `Не получилось: ${plural(failed, "вещь", "вещи", "вещей")} — причины ниже.`
      : mode === "apply"
        ? "Всё записано. Передумаете — верну как было."
        : "Всё на прежних местах.";
  $("result-note").textContent = note;
  const list = $("failures");
  list.replaceChildren(...result.failed.map((f) => el("p", "", `${f.name} — ${f.reason}`)));
  const revert = $("revert");
  revert.hidden = !result.can_undo;
  revert.textContent = mode === "apply" ? "вернуть как было" : "попробовать ещё раз";
  show("done");
  $("finish").focus();
}

async function undoLast() {
  if (state.busy) return;
  state.busy = true;
  try {
    renderDone(await invoke("undo_last"), "undo");
  } catch (e) {
    toast(e);
  } finally {
    state.busy = false;
  }
}

async function goHome() {
  show("home");
  await loadStart().catch(toast);
}

// ---------- Перетаскивание папки на визитку ----------

function listenDrops() {
  const subtitle = $("subtitle");
  const calm = subtitle.textContent;
  const reset = () => {
    card.classList.remove("is-dragging");
    subtitle.textContent = calm;
  };
  tauri.webview.getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (state.view !== "home" || state.busy) return;
    if (payload.type === "enter" || payload.type === "over") {
      card.classList.add("is-dragging");
      subtitle.textContent = "отпускайте — разложу эту папку";
    } else if (payload.type === "leave") {
      reset();
    } else if (payload.type === "drop") {
      reset();
      if (payload.paths && payload.paths.length) planFor(payload.paths);
    }
  });
}

// ---------- Перламутр: блик плывёт за курсором ----------

function followPointer() {
  let frame = 0;
  card.addEventListener("pointermove", (e) => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const x = e.clientX / innerWidth - 0.5;
      const y = e.clientY / innerHeight - 0.5;
      card.style.setProperty("--sx", `${(x * 6).toFixed(2)}%`);
      card.style.setProperty("--sy", `${(y * 6).toFixed(2)}%`);
    });
  });
}

// ---------- Клавиши ----------

function keys() {
  document.addEventListener("keydown", (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape" && state.view !== "home" && !state.busy) goHome();
    else if (e.key === "Enter" && state.view === "plan" && !e.target.closest?.(".shelf")) applyPlan();
    else if (e.key === "Enter" && state.view === "done") goHome();
    else if (state.view === "home" && /^[1-9]$/.test(e.key) && state.choices?.[e.key - 1]) planFor(state.choices[e.key - 1].paths);
    else if (state.view === "home" && mod && e.key.toLowerCase() === "z" && state.canUndo) undoLast();
  });
}

// ---------- Старт ----------

if (/Mac/.test(navigator.userAgent)) document.documentElement.classList.add("is-mac");
$("today").textContent = new Intl.DateTimeFormat("ru-RU", { day: "numeric", month: "long", year: "numeric" })
  .format(new Date())
  .replace(/\s?г\.?$/, "");
renderFoldersToggle();
$("folders").addEventListener("click", () => {
  state.folders = !state.folders;
  renderFoldersToggle();
});
$("rules").addEventListener("click", () => invoke("open_rules").catch(toast));
$("undo").addEventListener("click", undoLast);
$("back").addEventListener("click", goHome);
$("apply").addEventListener("click", applyPlan);
$("finish").addEventListener("click", goHome);
$("revert").addEventListener("click", undoLast);
keys();
followPointer();
listenDrops();
loadStart().catch(toast);

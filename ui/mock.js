// Подмена ядра для обычного браузера: чтобы дизайн можно было править без сборки Rust.
// В окне программы Tauri подставляет настоящий window.__TAURI__ раньше, и этот файл ничего не делает.

if (!window.__TAURI__) {
  const listeners = {};
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  let lastRun = null;

  const plan = {
    folder: "Рабочий стол",
    total: "37 вещей",
    size: "1,2 ГБ",
    count: 37,
    cloud: "Из облака скачается 975,6 МБ — эти файлы уезжают из папок iCloud.",
    shelves: [
      { title: "Скриншоты", place: "Изображения / Скриншоты · по месяцам", count: 14, size: "38,4 МБ", items: ["Снимок экрана 2026-10-30 в 05.54.33.png", "Снимок экрана 2026-10-29 в 22.10.07.png", "Снимок экрана 2026-10-28 в 11.02.41.png"] },
      { title: "С телефона", place: "Изображения / С телефона · по месяцам", count: 7, size: "13,1 МБ", items: ["IMG_2500.PNG", "IMG_2501.PNG", "IMG_2502.PNG"] },
      { title: "Рилсы", place: "Фильмы / Рилсы", count: 4, size: "25,0 МБ", items: ["Рилс — без подписок.mp4", "Рилс — как собран рилс.mp4"] },
      { title: "Договоры и счета", place: "Документы / Договоры и счета", count: 3, size: "1,4 МБ", items: ["Договор Павел.pdf", "Счёт 42.pdf", "Оферта.docx"] },
      { title: "Архивы", place: "Документы / Архивы", count: 4, size: "74,8 МБ", items: ["KSD_portfolio.zip", "Костянчик-конфиг.zip"] },
      { title: "Установщики", place: "Рабочий стол / На удаление / Установщики", count: 2, size: "975,6 МБ", items: ["Mimo-TikTok-36.5.4.apk", "TikTok_MC_1.0.3.apk"] },
      { title: "Дубли", place: "Рабочий стол / На удаление / Дубли", count: 2, size: "3,1 МБ", items: ["фото.jpg = Изображения / Картинки / фото.jpg", "Договор (1).pdf = Документы / Договоры и счета / Договор.pdf"] },
      { title: "Музыка и звук", place: "Музыка / Аудио", count: 1, size: "2,8 МБ", items: ["Drezly — 2AM Club.mp3"] },
      { title: "Заметки", place: "Документы / Заметки", count: 2, size: "29 КБ", items: ["ИНСТРУКЦИЯ.md", "идеи.txt"] },
    ],
    skipped: [
      { name: "Проекты", reason: "ярлык" },
      { name: "ТТ.command", reason: "кнопка-скрипт" },
      { name: "Чисто", reason: "внутри сейчас работает zsh" },
      { name: "videos", reason: "нужна для ТТ.command" },
      { name: "DragShift", reason: "рабочая папка программы DragShift" },
      { name: "Записи экрана", reason: "сюда система сохраняет скриншоты" },
    ],
  };

  const commands = {
    start: () => ({
      choices: [
        { label: "Рабочий стол", paths: ["/Users/me/Desktop"] },
        { label: "Загрузки", paths: ["/Users/me/Downloads"] },
        { label: "Документы", paths: ["/Users/me/Documents"] },
        { label: "Всё сразу", paths: ["/Users/me/Desktop", "/Users/me/Downloads", "/Users/me/Documents"] },
      ],
      last: lastRun,
      rules_error: null,
    }),
    make_plan: async () => (await wait(250), plan),
    apply_plan: async () => {
      for (let i = 0; i <= plan.count; i++) {
        (listeners.progress || []).forEach((cb) => cb({ payload: { done: i, total: plan.count } }));
        await wait(28);
      }
      lastRun = { folder: "Рабочий стол", count: plan.total };
      return { moved: plan.count, text: `Разложено: ${plan.total}.`, failed: [], can_undo: true };
    },
    undo_last: async () => (await wait(300), (lastRun = null), { moved: 37, text: "Вернул на место: 37 вещей.", failed: [], can_undo: false }),
    open_rules: () => null,
    reveal: () => null,
  };

  window.__TAURI__ = {
    core: { invoke: async (name, args) => commands[name](args) },
    event: {
      listen: async (name, cb) => {
        (listeners[name] ||= []).push(cb);
        return () => (listeners[name] = listeners[name].filter((x) => x !== cb));
      },
    },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => {}) }) },
  };
}

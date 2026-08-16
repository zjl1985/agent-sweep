import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "preact/hooks";
import { deleteSessions, scanSessions } from "./api";
import type { DeleteMode, ScanResult, SessionEntry } from "./types";
import {
  loadLocale,
  localizeDirName,
  localizeMessage,
  localizeTitle,
  saveLocale,
  strings,
  type Locale,
} from "./i18n";
import {
  ArchiveIcon,
  DatabaseIcon,
  FolderIcon,
  RefreshIcon,
  SearchIcon,
  SettingsIcon,
  TrashIcon,
} from "./components/Icons";
import "./App.css";

type StatusFilter = "all" | "active" | "archived";
type SortKey = "size" | "updated";
type AgeFilter = 0 | 7 | 30 | 90;

const AGE_VALUES: AgeFilter[] = [0, 7, 30, 90];

const MODE_STORAGE_KEY = "codex-clear:delete-mode";
const DAY_MS = 86_400_000;

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1024;
    unit += 1;
  } while (value >= 1024 && unit < units.length - 1);
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function formatDateTime(ms: number | null): string {
  if (ms === null) return "—";
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
    date.getDate(),
  )} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function loadDefaultMode(): DeleteMode {
  return localStorage.getItem(MODE_STORAGE_KEY) === "permanent"
    ? "permanent"
    : "trash";
}

export default function App() {
  const [locale, setLocale] = useState<Locale>(() => loadLocale());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [ageFilter, setAgeFilter] = useState<AgeFilter>(0);
  const [sortBy, setSortBy] = useState<SortKey>("size");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [activeIndex, setActiveIndex] = useState(0);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [dialogMode, setDialogMode] = useState<DeleteMode>("trash");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [defaultMode, setDefaultMode] = useState<DeleteMode>("trash");

  const t = strings[locale];

  const searchRef = useRef<HTMLInputElement>(null);
  const confirmRef = useRef<HTMLDialogElement>(null);
  const settingsRef = useRef<HTMLDialogElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }, [locale]);

  useEffect(() => {
    setDefaultMode(loadDefaultMode());
  }, []);

  const switchLocale = useCallback((next: Locale) => {
    setLocale(next);
    saveLocale(next);
  }, []);

  const doScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const result = await scanSessions();
      setScan(result);
      setSelected(new Set());
      setActiveIndex(0);
      setNotice(
        strings[locale].scanNotice(
          result.sessions.length,
          formatSize(result.sessionsBytes),
        ),
      );
    } catch (cause) {
      setError(String(cause));
    } finally {
      setScanning(false);
    }
  }, [locale]);

  useEffect(() => {
    void doScan();
  }, [doScan]);

  const visibleSessions = useMemo(() => {
    const sessions = scan?.sessions ?? [];
    const keyword = query.trim().toLowerCase();
    const cutoff = ageFilter === 0 ? 0 : Date.now() - ageFilter * DAY_MS;
    return sessions.filter((session) => {
      if (statusFilter === "active" && session.archived) return false;
      if (statusFilter === "archived" && !session.archived) return false;
      if (cutoff !== 0 && session.updatedAtMs >= cutoff) return false;
      if (keyword === "") return true;
      const haystack = [
        session.title,
        session.workspace ?? "",
        session.sessionId,
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(keyword);
    });
  }, [scan, query, statusFilter, ageFilter]);

  const sortedSessions = useMemo(() => {
    const sessions = [...visibleSessions];
    sessions.sort((a, b) =>
      sortBy === "size"
        ? b.sizeBytes - a.sizeBytes || b.updatedAtMs - a.updatedAtMs
        : b.updatedAtMs - a.updatedAtMs || b.sizeBytes - a.sizeBytes,
    );
    return sessions;
  }, [visibleSessions, sortBy]);

  const maxBytes = useMemo(
    () => sortedSessions.reduce((max, s) => Math.max(max, s.sizeBytes), 1),
    [sortedSessions],
  );

  const selectedSessions = useMemo(
    () => (scan?.sessions ?? []).filter((s) => selected.has(s.sessionId)),
    [scan, selected],
  );
  const selectedBytes = selectedSessions.reduce(
    (sum, s) => sum + s.sizeBytes,
    0,
  );

  const toggleSelected = useCallback((sessionId: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(sessionId)) {
        next.delete(sessionId);
      } else {
        next.add(sessionId);
      }
      return next;
    });
  }, []);

  const selectAllVisible = useCallback(() => {
    setSelected(new Set(sortedSessions.map((session) => session.sessionId)));
  }, [sortedSessions]);

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  const openConfirm = useCallback(() => {
    setDialogMode(defaultMode);
    setConfirmOpen(true);
    confirmRef.current?.showModal();
  }, [defaultMode]);

  const closeConfirm = useCallback(() => {
    setConfirmOpen(false);
    confirmRef.current?.close();
  }, []);

  const openSettings = useCallback(() => {
    setSettingsOpen(true);
    settingsRef.current?.showModal();
  }, []);

  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
    settingsRef.current?.close();
  }, []);

  const saveDefaultMode = useCallback((mode: DeleteMode) => {
    setDefaultMode(mode);
    localStorage.setItem(MODE_STORAGE_KEY, mode);
  }, []);

  const performDelete = useCallback(async () => {
    setDeleting(true);
    try {
      const result = await deleteSessions({
        sessionIds: [...selected],
        mode: dialogMode,
      });
      const summary = strings[locale].deletedNotice(
        result.deletedCount,
        formatSize(result.freedBytes),
      );
      if (result.failed.length > 0) {
        setError(
          summary +
            strings[locale].deleteFailedSuffix(
              result.failed.length,
              localizeMessage(result.failed[0].message, locale),
            ),
        );
      } else {
        setNotice(summary);
      }
      closeConfirm();
      await doScan();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setDeleting(false);
    }
  }, [selected, dialogMode, closeConfirm, doScan, locale]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      if (meta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (meta && event.key === ",") {
        event.preventDefault();
        openSettings();
        return;
      }
      if (confirmOpen || settingsOpen) return;
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT")
      ) {
        return;
      }
      if (sortedSessions.length === 0) return;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveIndex((index) => Math.min(index + 1, sortedSessions.length - 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveIndex((index) => Math.max(index - 1, 0));
      } else if (event.key === "Home") {
        event.preventDefault();
        setActiveIndex(0);
      } else if (event.key === "End") {
        event.preventDefault();
        setActiveIndex(sortedSessions.length - 1);
      } else if (event.key === " " || event.key === "Enter") {
        const session = sortedSessions[activeIndex];
        if (session) {
          event.preventDefault();
          toggleSelected(session.sessionId);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    sortedSessions,
    activeIndex,
    toggleSelected,
    confirmOpen,
    settingsOpen,
    openSettings,
  ]);

  useEffect(() => {
    const row = listRef.current?.querySelector<HTMLElement>(
      `[data-index="${activeIndex}"]`,
    );
    row?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, sortedSessions]);

  const activeCount = scan?.activeCount ?? 0;
  const archivedCount = scan?.archivedCount ?? 0;

  const ageLabel = (value: AgeFilter): string =>
    value === 0
      ? t.ageAny
      : value === 7
        ? t.age7
        : value === 30
          ? t.age30
          : t.age90;

  return (
    <div class="app-shell">
      <header class="app-header">
        <div class="brand">
          <h1 class="brand-title">CodexClear</h1>
          <p class="brand-sub">{t.brandSubtitle}</p>
        </div>
        <div class="header-actions">
          <div class="locale-switch" role="group" aria-label="Language / 语言">
            <button
              class={`chip ${locale === "en" ? "chip-active" : ""}`}
              onClick={() => switchLocale("en")}
              aria-pressed={locale === "en"}
              title="English"
            >
              EN
            </button>
            <button
              class={`chip ${locale === "zh" ? "chip-active" : ""}`}
              onClick={() => switchLocale("zh")}
              aria-pressed={locale === "zh"}
              title="中文"
            >
              中
            </button>
          </div>
          <button
            class="ghost-button"
            onClick={openSettings}
            title={t.settingsAria}
          >
            <SettingsIcon /> {t.settings}
          </button>
          <button
            class="ghost-button"
            onClick={() => void doScan()}
            disabled={scanning}
          >
            <RefreshIcon /> {scanning ? t.scanning : t.rescan}
          </button>
        </div>
      </header>

      {error && (
        <div class="banner banner-error" role="alert">
          {localizeMessage(error, locale)}
          <button class="banner-close" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      )}
      {scan && scan.issues.length > 0 && (
        <div class="banner banner-warning">
          {scan.issues
            .map((issue) => localizeMessage(issue, locale))
            .join(locale === "zh" ? "；" : "; ")}
        </div>
      )}

      <section class="stats-row" aria-label={t.statsLabel}>
        <div class="stat stat-primary">
          <span class="stat-value">{formatSize(scan?.sessionsBytes ?? 0)}</span>
          <span class="stat-label">
            {t.totalSize(scan?.sessions.length ?? 0)}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{activeCount}</span>
          <span class="stat-label">
            {t.activeLabel(formatSize(scan?.activeBytes ?? 0))}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{archivedCount}</span>
          <span class="stat-label">
            {t.archivedLabel(formatSize(scan?.archivedBytes ?? 0))}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{formatSize(scan?.codexTotalBytes ?? 0)}</span>
          <span class="stat-label">{t.codexTotal}</span>
        </div>
      </section>

      {scan && scan.otherDirs.length > 0 && (
        <section class="overview-row" aria-label={t.otherUsageLabel}>
          <DatabaseIcon size={14} />
          <span class="overview-title">{t.otherUsage}</span>
          <div class="overview-chips">
            {scan.otherDirs.map((dir) => (
              <span class="overview-chip" title={dir.note}>
                {localizeDirName(dir.name, locale)}{" "}
                {formatSize(dir.sizeBytes)}
              </span>
            ))}
          </div>
        </section>
      )}

      <section class="toolbar" aria-label={t.toolbarLabel}>
        <div class="search-box">
          <SearchIcon size={14} />
          <input
            ref={searchRef}
            type="search"
            placeholder={t.searchPlaceholder}
            value={query}
            onInput={(event) => setQuery(event.currentTarget.value)}
            aria-label={t.searchLabel}
          />
        </div>
        <div class="chip-group" role="group" aria-label={t.statusFilterLabel}>
          {(
            [
              ["all", t.chipAll(scan?.sessions.length ?? 0)],
              ["active", t.chipActive(activeCount)],
              ["archived", t.chipArchived(archivedCount)],
            ] as [StatusFilter, string][]
          ).map(([value, label]) => (
            <button
              class={`chip ${statusFilter === value ? "chip-active" : ""}`}
              onClick={() => setStatusFilter(value)}
              aria-pressed={statusFilter === value}
            >
              {label}
            </button>
          ))}
        </div>
        <select
          class="select"
          value={ageFilter}
          onChange={(event) =>
            setAgeFilter(Number(event.currentTarget.value) as AgeFilter)
          }
          aria-label={t.ageFilterLabel}
        >
          {AGE_VALUES.map((value) => (
            <option value={value}>{ageLabel(value)}</option>
          ))}
        </select>
        <select
          class="select"
          value={sortBy}
          onChange={(event) => setSortBy(event.currentTarget.value as SortKey)}
          aria-label={t.sortLabel}
        >
          <option value="size">{t.sortSize}</option>
          <option value="updated">{t.sortUpdated}</option>
        </select>
        <div class="toolbar-spacer" />
        <button
          class="ghost-button"
          onClick={selectAllVisible}
          disabled={sortedSessions.length === 0}
        >
          {t.selectAll(sortedSessions.length)}
        </button>
      </section>

      <div
        class="session-list"
        role="listbox"
        aria-label={t.listLabel}
        aria-multiselectable="true"
        ref={listRef}
        tabIndex={0}
      >
        {sortedSessions.length === 0 && (
          <div class="empty-state">
            {scanning ? t.emptyScanning : t.emptyNoMatch}
          </div>
        )}
        {sortedSessions.map((session, index) => (
          <SessionRow
            key={session.sessionId}
            session={session}
            index={index}
            active={index === activeIndex}
            checked={selected.has(session.sessionId)}
            maxBytes={maxBytes}
            locale={locale}
            onToggle={() => toggleSelected(session.sessionId)}
          />
        ))}
      </div>

      <footer class="selection-bar">
        <span
          class={`selection-hint ${selected.size > 0 ? "cluster-hidden" : ""}`}
        >
          {t.hint}
        </span>
        <div
          class={`selection-cluster ${selected.size > 0 ? "" : "cluster-hidden"}`}
        >
          <span class="selection-summary">
            {t.selectedSummary(selected.size, formatSize(selectedBytes))}
          </span>
          <button class="ghost-button" onClick={clearSelection}>
            {t.clearSelection}
          </button>
          <button
            class="danger-button"
            onClick={openConfirm}
            disabled={deleting}
          >
            <TrashIcon /> {t.deleteSelected}
          </button>
        </div>
      </footer>

      <div class="sr-only" role="status" aria-live="polite">
        {notice}
      </div>

      <dialog
        ref={confirmRef}
        class="dialog"
        onClose={() => setConfirmOpen(false)}
      >
        <h2>{t.confirmTitle}</h2>
        <p>
          {t.confirmBody(selected.size, formatSize(selectedBytes))}
        </p>
        <fieldset class="mode-fieldset">
          <legend>{t.mode}</legend>
          <label>
            <input
              type="radio"
              name="confirm-mode"
              checked={dialogMode === "trash"}
              onChange={() => setDialogMode("trash")}
            />
            {t.trashOption}
          </label>
          <label>
            <input
              type="radio"
              name="confirm-mode"
              checked={dialogMode === "permanent"}
              onChange={() => setDialogMode("permanent")}
            />
            {t.permanentOption}
          </label>
        </fieldset>
        <div class="dialog-actions">
          <button class="ghost-button" onClick={closeConfirm}>
            {t.cancel}
          </button>
          <button
            class={dialogMode === "permanent" ? "danger-button" : "primary-button"}
            onClick={() => void performDelete()}
            disabled={deleting}
          >
            {deleting
              ? t.working
              : dialogMode === "permanent"
                ? t.deletePermanently
                : t.moveTrash}
          </button>
        </div>
      </dialog>

      <dialog
        ref={settingsRef}
        class="dialog"
        onClose={() => setSettingsOpen(false)}
      >
        <h2>{t.settingsTitle}</h2>
        <fieldset class="mode-fieldset">
          <legend>{t.defaultMode}</legend>
          <label>
            <input
              type="radio"
              name="default-mode"
              checked={defaultMode === "trash"}
              onChange={() => saveDefaultMode("trash")}
            />
            {t.trashRecommended}
          </label>
          <label>
            <input
              type="radio"
              name="default-mode"
              checked={defaultMode === "permanent"}
              onChange={() => saveDefaultMode("permanent")}
            />
            {t.permanent}
          </label>
        </fieldset>
        <p class="settings-note">{t.settingsNote}</p>
        <div class="dialog-actions">
          <button class="primary-button" onClick={closeSettings}>
            {t.done}
          </button>
        </div>
      </dialog>
    </div>
  );
}

interface SessionRowProps {
  session: SessionEntry;
  index: number;
  active: boolean;
  checked: boolean;
  maxBytes: number;
  locale: Locale;
  onToggle: () => void;
}

function SessionRow({
  session,
  index,
  active,
  checked,
  maxBytes,
  locale,
  onToggle,
}: SessionRowProps) {
  const barPercent = Math.max(
    2,
    Math.round((session.sizeBytes / maxBytes) * 100),
  );
  const title = localizeTitle(session.title, locale);
  return (
    <div
      class={`session-row ${active ? "row-active" : ""} ${
        checked ? "row-checked" : ""
      }`}
      role="option"
      aria-selected={checked}
      data-index={index}
      onClick={onToggle}
    >
      <input
        type="checkbox"
        checked={checked}
        onClick={(event) => event.stopPropagation()}
        onInput={onToggle}
        aria-label={strings[locale].selectRow(title)}
        tabIndex={-1}
      />
      <div class="row-main">
        <div class="row-title-line">
          <span class="row-title" title={title}>
            {title}
          </span>
          {session.archived && (
            <span class="badge badge-archived">
              <ArchiveIcon size={11} /> {strings[locale].badgeArchived}
            </span>
          )}
        </div>
        <div class="row-meta-line">
          {session.workspace && (
            <span class="row-workspace" title={session.workspace}>
              <FolderIcon size={11} /> {session.workspace}
            </span>
          )}
          <span class="row-id">{session.sessionId.slice(0, 8)}…</span>
        </div>
      </div>
      <div class="row-date">{formatDateTime(session.updatedAtMs)}</div>
      <div class="row-size">
        {formatSize(session.sizeBytes)}
        <div class="size-bar" aria-hidden="true">
          <div class="size-bar-fill" style={{ width: `${barPercent}%` }} />
        </div>
      </div>
    </div>
  );
}

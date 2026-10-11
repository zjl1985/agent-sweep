import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "preact/hooks";
import { archiveSessions, deleteSessions, scanSessions } from "./api";
import type { DeleteMode, ScanResult, SessionEntry, SessionTarget } from "./types";
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
  MoonIcon,
  RefreshIcon,
  SearchIcon,
  SettingsIcon,
  SunIcon,
  TrashIcon,
} from "./components/Icons";
import { UsageComparison } from "./components/UsageComparison";
import "./App.css";

type StatusFilter = "all" | "active" | "archived";
type SortKey = "size" | "updated";
type AgeFilter = 0 | 7 | 30 | 90;
type Theme = "light" | "dark";

const AGE_VALUES: AgeFilter[] = [0, 7, 30, 90];

// Keep the legacy keys so an in-place upgrade retains the user's settings.
const MODE_STORAGE_KEY = "codex-clear:delete-mode";
const THEME_STORAGE_KEY = "codex-clear:theme";
const PROVIDER_STORAGE_KEY = "agent-sweep:provider";
const DAY_MS = 86_400_000;

function selectionKey(session: SessionEntry): string {
  return JSON.stringify([session.providerId, session.sessionKey]);
}

function sessionTarget(session: SessionEntry): SessionTarget {
  return { providerId: session.providerId, sessionKey: session.sessionKey };
}

function supportsDelete(session: SessionEntry, mode: DeleteMode): boolean {
  return mode === "trash"
    ? session.capabilities.deleteTrash
    : session.capabilities.deletePermanent;
}

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

function loadTheme(): Theme {
  const stored = localStorage.getItem(THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export default function App() {
  const [locale, setLocale] = useState<Locale>(() => loadLocale());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [providerFilter, setProviderFilter] = useState(() => localStorage.getItem(PROVIDER_STORAGE_KEY) || "codex");
  const [usageOpen, setUsageOpen] = useState(false);
  const [ageFilter, setAgeFilter] = useState<AgeFilter>(0);
  const [sortBy, setSortBy] = useState<SortKey>("size");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [activeIndex, setActiveIndex] = useState(0);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [dialogMode, setDialogMode] = useState<DeleteMode>("trash");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [defaultMode, setDefaultMode] = useState<DeleteMode>("trash");
  const [theme, setTheme] = useState<Theme>(() => loadTheme());

  const t = strings[locale];

  const searchRef = useRef<HTMLInputElement>(null);
  const confirmRef = useRef<HTMLDialogElement>(null);
  const settingsRef = useRef<HTMLDialogElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }, [locale]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Follow the OS appearance until the user picks a theme explicitly.
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (localStorage.getItem(THEME_STORAGE_KEY) === null) {
        setTheme(media.matches ? "dark" : "light");
      }
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme((current) => {
      const next = current === "dark" ? "light" : "dark";
      localStorage.setItem(THEME_STORAGE_KEY, next);
      return next;
    });
  }, []);

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
      setProviderFilter((current) =>
        result.providers.some((provider) => provider.id === current)
          ? current
          : result.providers.find((provider) => provider.id === "codex")?.id
            ?? result.providers[0]?.id ?? "codex",
      );
      setSelected(new Set());
      setActiveIndex(0);
      setNotice(null);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setScanning(false);
    }
  }, [locale]);

  useEffect(() => {
    void doScan();
  }, [doScan]);

  useEffect(() => {
    localStorage.setItem(PROVIDER_STORAGE_KEY, providerFilter);
  }, [providerFilter]);

  const switchProvider = (id: string) => {
    setProviderFilter(id);
    setSelected(new Set());
    setStatusFilter("all");
    setQuery("");
    setAgeFilter(0);
    setActiveIndex(0);
    setUsageOpen(false);
    setNotice(null);
  };

  const provider = scan?.providers.find((entry) => entry.id === providerFilter);
  const providerSessions = useMemo(
    () => (scan?.sessions ?? []).filter((session) => session.providerId === providerFilter),
    [scan, providerFilter],
  );
  const usage = (scan?.otherDirs ?? []).filter(
    (dir) => dir.providerId === providerFilter && dir.sizeBytes > 0,
  );
  const activeSessions = providerSessions.filter((session) => !session.archived);
  const archivedSessions = providerSessions.filter((session) => session.archived);
  const sumBytes = (sessions: SessionEntry[]) => sessions.reduce((sum, session) => sum + session.sizeBytes, 0);

  const visibleSessions = useMemo(() => {
    const sessions = providerSessions;
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
        session.providerName,
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(keyword);
    });
  }, [providerSessions, query, statusFilter, ageFilter]);

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
    () => providerSessions.filter((s) => selected.has(selectionKey(s))),
    [providerSessions, selected],
  );
  const selectedBytes = selectedSessions.reduce(
    (sum, s) => sum + s.sizeBytes,
    0,
  );

  const archivableSessions = useMemo(
    () => selectedSessions.filter((s) => !s.archived && s.capabilities.archive),
    [selectedSessions],
  );
  const canTrash = selectedSessions.length > 0 &&
    selectedSessions.every((s) => supportsDelete(s, "trash"));
  const canDeletePermanently = selectedSessions.length > 0 &&
    selectedSessions.every((s) => supportsDelete(s, "permanent"));
  const canDelete = canTrash || canDeletePermanently;
  const canDeleteInDialog = dialogMode === "trash" ? canTrash : canDeletePermanently;

  const allFilteredSelected =
    sortedSessions.length > 0 &&
    sortedSessions.every((session) => selected.has(selectionKey(session)));

  const toggleSelected = useCallback((key: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

  const selectAllVisible = useCallback(() => {
    setSelected(new Set(sortedSessions.map(selectionKey)));
  }, [sortedSessions]);

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  const openConfirm = useCallback(() => {
    if (!canDelete || deleting || archiving) return;
    setDialogMode(
      defaultMode === "trash"
        ? canTrash ? "trash" : "permanent"
        : canDeletePermanently ? "permanent" : "trash",
    );
    setConfirmOpen(true);
    confirmRef.current?.showModal();
  }, [defaultMode, canDelete, canTrash, canDeletePermanently, deleting, archiving]);

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
    if (!canDeleteInDialog || deleting || archiving) return;
    setDeleting(true);
    try {
      const result = await deleteSessions({
        targets: selectedSessions.map(sessionTarget),
        mode: dialogMode,
      });
      closeConfirm();
      await doScan();
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
    } catch (cause) {
      setError(String(cause));
    } finally {
      setDeleting(false);
    }
  }, [selectedSessions, canDeleteInDialog, deleting, archiving, dialogMode, closeConfirm, doScan, locale]);

  const performArchive = useCallback(async () => {
    if (archivableSessions.length === 0 || archiving || deleting) return;
    setArchiving(true);
    try {
      const result = await archiveSessions({
        targets: archivableSessions.map(sessionTarget),
      });
      await doScan();
      const summary = strings[locale].archivedNotice(result.archivedCount);
      if (result.failed.length > 0) {
        setError(
          summary +
            strings[locale].archiveFailedSuffix(
              result.failed.length,
              localizeMessage(result.failed[0].message, locale),
            ),
        );
      } else {
        setNotice(summary);
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setArchiving(false);
    }
  }, [archivableSessions, archiving, deleting, doScan, locale]);

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
          target.tagName === "SELECT" ||
          target.closest("button, summary"))
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
          toggleSelected(selectionKey(session));
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

  const activeCount = activeSessions.length;
  const archivedCount = archivedSessions.length;

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
          <div class="brand-heading">
            <h1 class="brand-title">AgentSweep</h1>
            {scan && scan.providers.length > 0 && (
              <select
                class="select provider-select"
                value={providerFilter}
                onChange={(event) => switchProvider(event.currentTarget.value)}
                aria-label={t.providerFilterLabel}
                disabled={deleting || archiving || scanning}
              >
                {scan.providers.map((entry) => (
                  <option key={entry.id} value={entry.id}>{entry.name}</option>
                ))}
              </select>
            )}
          </div>
          <p class="brand-sub">{t.brandSubtitle}</p>
        </div>
        <div class="header-actions">
          <button
            class="icon-button"
            onClick={toggleTheme}
            title={theme === "dark" ? t.toLightMode : t.toDarkMode}
            aria-label={theme === "dark" ? t.toLightMode : t.toDarkMode}
          >
            {theme === "dark" ? <SunIcon size={15} /> : <MoonIcon size={15} />}
          </button>
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
            class="icon-button"
            onClick={openSettings}
            title={t.settingsAria}
            aria-label={t.settingsAria}
          >
            <SettingsIcon size={15} />
          </button>
          <button
            class={`icon-button ${scanning ? "spinning" : ""}`}
            onClick={() => void doScan()}
            disabled={scanning}
            title={scanning ? t.scanning : t.rescan}
            aria-label={scanning ? t.scanning : t.rescan}
          >
            <RefreshIcon size={15} />
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
      {provider && provider.issues.length > 0 && (
        <div class="banner banner-warning">
          {provider.issues
            .map((issue) => localizeMessage(issue, locale))
            .join(locale === "zh" ? "；" : "; ")}
        </div>
      )}

      {scan && scan.providers.length > 1 && <UsageComparison scan={scan} current={providerFilter} locale={locale} disabled={deleting || archiving || scanning} onSelect={switchProvider} formatSize={formatSize} />}

      <section class="stats-row" aria-label={t.statsLabel}>
        <div class="stat stat-primary">
          <span class="stat-value">{formatSize(sumBytes(providerSessions))}</span>
          <span class="stat-label">
            {t.totalSize(providerSessions.length)}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{activeCount}</span>
          <span class="stat-label">
            {t.activeLabel(formatSize(sumBytes(activeSessions)))}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{archivedCount}</span>
          <span class="stat-label">
            {t.archivedLabel(formatSize(sumBytes(archivedSessions)))}
          </span>
        </div>
        <div class="stat">
          <span class="stat-value">{formatSize(provider?.totalBytes ?? 0)}</span>
          <span class="stat-label">{t.providerTotal(provider?.name ?? "Agent")}</span>
        </div>
      </section>

      {usage.length > 0 && (
        <section class="usage-section" aria-label={t.otherUsageLabel}>
          <button
            class="usage-toggle"
            aria-expanded={usageOpen}
            aria-controls="usage-details"
            onClick={() => setUsageOpen((open) => !open)}
          >
            <DatabaseIcon size={14} />
            <span>{t.otherUsage}</span>
            <span>{formatSize(usage.reduce((sum, dir) => sum + dir.sizeBytes, 0))}</span>
            <span class="usage-chevron" aria-hidden="true">{usageOpen ? "⌃" : "⌄"}</span>
          </button>
          {usageOpen && (
            <div class="overview-chips usage-details" id="usage-details">
              {usage.map((dir) => (
                <span class="overview-chip" key={dir.name} title={dir.note}>
                  {localizeDirName(dir.name, locale)} {formatSize(dir.sizeBytes)}
                </span>
              ))}
            </div>
          )}
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
              ["all", t.chipAll(providerSessions.length)],
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
        {/* Always rendered (hidden when empty) so Select all never shifts. */}
        <button
          class={`ghost-button ${selected.size > 0 ? "" : "cluster-hidden"}`}
          onClick={clearSelection}
          tabIndex={selected.size > 0 ? 0 : -1}
          aria-hidden={selected.size === 0}
        >
          {t.clearSelected(selected.size)}
        </button>
        <button
          class="ghost-button"
          onClick={selectAllVisible}
          disabled={sortedSessions.length === 0 || allFilteredSelected}
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
            key={selectionKey(session)}
            session={session}
            index={index}
            active={index === activeIndex}
            checked={selected.has(selectionKey(session))}
            maxBytes={maxBytes}
            locale={locale}
            onToggle={() => toggleSelected(selectionKey(session))}
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
          {selectedSessions.some((session) => session.capabilities.archive) && (
            <button
              class="ghost-button"
              onClick={() => void performArchive()}
              disabled={archiving || deleting || archivableSessions.length === 0}
            >
              <ArchiveIcon /> {t.archiveSelected}
            </button>
          )}
          <button
            class="danger-button"
            onClick={openConfirm}
            disabled={deleting || archiving || !canDelete}
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
              disabled={!canTrash}
              checked={dialogMode === "trash"}
              onChange={() => setDialogMode("trash")}
            />
            {t.trashOption}
          </label>
          <label>
            <input
              type="radio"
              name="confirm-mode"
              disabled={!canDeletePermanently}
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
            disabled={deleting || archiving || !canDeleteInDialog}
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
